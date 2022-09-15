use crate::{ShellCmdApi, CommonEnv};
use com::api::NET_MTU;
use xous_ipc::String;
#[cfg(any(target_os = "none", target_os = "xous"))]
use net::XousServerId;
use net::NetPingCallback;
use xous::MessageEnvelope;
use num_traits::*;
use std::net::{IpAddr, TcpStream, TcpListener};
use std::io::Write;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::thread;
use std::sync::mpsc;
use dns::Dns; // necessary to work around https://github.com/rust-lang/rust/issues/94182
#[cfg(feature="ditherpunk")]
use std::str::FromStr;
#[cfg(feature="ditherpunk")]
use gam::DecodePng;
#[cfg(feature="tls")]
use std::convert::TryInto;
#[cfg(feature="tls")]
use tungstenite::{WebSocket, stream::MaybeTlsStream};
#[cfg(feature="perfcounter")]
use utralib::generated::*;
#[cfg(feature="perfcounter")]
use core::num::{NonZeroU8, NonZeroU16};
#[cfg(feature="perfcounter")]
use std::cell::RefCell;

#[cfg(feature="perfcounter")]
const BUFLEN: usize = 1024 * 512;

#[cfg(feature="perfcounter")]
#[repr(C)]
pub struct PerfLogEntry {
    code: u32,
    ts: u32,
}

#[cfg(feature="perfcounter")]
pub struct PerfMgr<'a> {
    data: RefCell::<&'a mut [PerfLogEntry]>,
    perf_csr: AtomicCsr<u32>,
    event_csr: AtomicCsr<u32>,
    log_index: RefCell<usize>,
    ts_cont: RefCell<u64>,
    // these are expressed as raw values to be written to hardware
    saturation_limit: u64,
    prescaler: u16,
    saturate: bool,
    event_bit_width: u8,
    // debug fields
    dbg_buf_count: RefCell<u32>,
}
#[cfg(feature="perfcounter")]
impl <'a> PerfMgr<'a> {
    pub fn new(
        log_ptr: *mut u8,
        perf_csr: AtomicCsr<u32>,
        event_csr: AtomicCsr<u32>,
    ) -> Self {
        let data =
            unsafe{
                core::slice::from_raw_parts_mut(
                    log_ptr as *mut PerfLogEntry,
                    BUFLEN / core::mem::size_of::<PerfLogEntry>()
                )
            };
        // initialize the buffer region when the manager is created
        for d in data.iter_mut() {
            d.code = 0;
            d.ts = 0;
        }
        Self {
            data: RefCell::new(data),
            perf_csr,
            event_csr,
            log_index: RefCell::new(0),
            ts_cont: RefCell::new(0),
            saturation_limit: 0xffff_ffff,
            prescaler: 0, // 1 clock per sample
            event_bit_width: 31, // 32 bits width
            saturate: true,
            dbg_buf_count: RefCell::new(0),
        }
    }

    /// Sets the saturation limit for the performance counter. Units in clock cycles.
    /// If `None`, the counter will freely rollover without stopping the performance counting process.
    #[allow(dead_code)]
    pub fn sat_limit(&mut self, limit: Option<u64>) {
        if let Some(l) = limit {
            self.saturation_limit = l;
            self.saturate = true;
        } else {
            self.saturate = false;
            self.saturation_limit = u64::MAX;
        }
    }

    /// Zero is an invalid value for clocks per sample. Hence NonZeroU16 type.
    #[allow(dead_code)]
    pub fn clocks_per_sample(&mut self, cps: NonZeroU16) {
        self.prescaler = cps.get() - 1;
    }

    /// Sets the width of the event code. This is enforced by hardware, software is free to pass in a code that is too large.
    /// Excess bits will be ignored, starting from the MSB side. A bitwdith of 0 is illegal. A bitwidth larger than 32 is set to 32.
    #[allow(dead_code)]
    pub fn code_bitwidth(&mut self, bitwidth: NonZeroU8) {
        let bw = bitwidth.get();
        if bw > 32 {
            self.event_bit_width = 31;
        } else {
            self.event_bit_width = bw - 1;
        }
    }

    pub fn stop_and_reset(&self) {
        for d in self.data.borrow_mut().iter_mut() {
            d.code = 0;
            d.ts = 0;
        }
        self.perf_csr.wfo(utra::perfcounter::RUN_STOP, 1);
        while self.perf_csr.rf(utra::perfcounter::STATUS_READABLE) == 1 {
            let i = self.perf_csr.r(utra::perfcounter::EVENT_INDEX); // this advances the FIFO until it is empty
            log::warn!("FIFO was not drained before reset: {}", i);
        }
        // stop the counter if it would rollover
        self.perf_csr.wo(utra::perfcounter::SATURATE_LIMIT0, self.saturation_limit as u32);
        self.perf_csr.wo(utra::perfcounter::SATURATE_LIMIT1, (self.saturation_limit >> 32) as u32);

        // configure the system
        self.perf_csr.wo(utra::perfcounter::CONFIG,
            self.perf_csr.ms(utra::perfcounter::CONFIG_PRESCALER, self.prescaler as u32)
            | self.perf_csr.ms(utra::perfcounter::CONFIG_SATURATE, if self.saturate {1} else {0})
            | self.perf_csr.ms(utra::perfcounter::CONFIG_EVENT_WIDTH_MINUS_ONE, self.event_bit_width as u32)
        );
        self.log_index.replace(0);
        self.ts_cont.replace(0);
    }

    pub fn start(&self) {
        self.perf_csr.wfo(utra::perfcounter::RUN_RESET_RUN, 1);
    }

    /// This function is convenient, but the overhead of the checking adds a lot of cache line noise to the data
    /// If you are looking to get very cycle-accurate counts, use `log_event_unchecked` with manual flush calls
    ///
    /// returns Ok(()) if the event could be logged
    /// returns an Err if the performance buffer would overflow
    #[inline(always)]
    pub fn log_event(&self, code: u32) -> Result::<(), xous::Error> {
        self.flush_if_full()?;
        self.event_csr.wfo(utra::event_source1::PERFEVENT_CODE,
                code);
        Ok(())
    }

    #[allow(dead_code)]
    #[inline(always)]
    pub fn log_event_unchecked(&self, code: u32) {
        self.event_csr.wfo(utra::event_source1::PERFEVENT_CODE,
            code);
    }
    #[allow(dead_code)]
    pub fn flush(&self) -> Result::<(), xous::Error> {
        let mut oom = false;
        // stop the counter
        self.perf_csr.wfo(utra::perfcounter::RUN_STOP, 1);
        // copy over the events to the long term buffer
        let mut ts_offset = 0;
        let mut expected_i = 0;
        while self.perf_csr.rf(utra::perfcounter::STATUS_READABLE) == 1 {
            if *self.log_index.borrow() < self.data.borrow().len() {
                self.dbg_buf_count.replace(self.dbg_buf_count.take() + 1); // this tracks total entries copied into the perfbuf
                (*self.data.borrow_mut())[*self.log_index.borrow()].code = self.perf_csr.r(utra::perfcounter::EVENT_RAW0);
                ts_offset = self.perf_csr.r(utra::perfcounter::EVENT_RAW1) as u32;
                (*self.data.borrow_mut())[*self.log_index.borrow()].ts = *self.ts_cont.borrow() as u32 + ts_offset;

                let i = self.perf_csr.r(utra::perfcounter::EVENT_INDEX); // this advances the FIFO
                if i != expected_i & 0xfff {
                    log::info!("i {} != expected_i {}", i, expected_i);
                }
                expected_i += 1;
                self.log_index.replace(self.log_index.take() + 1);
            } else {
                oom = true;
                break;
            }
        }
        // update the timestamp continuation field with the last timestamp seen; the next line
        // resets the timestamp counter to 0 again.
        self.ts_cont.replace(self.ts_cont.take() + ts_offset as u64);
        // restart the counter
        if !oom {
            // duplicate this code down here because we want the reset and log to be as close as possible to the return statement
            self.perf_csr.wfo(utra::perfcounter::RUN_RESET_RUN, 1);
            Ok(())
        } else {
            self.perf_csr.wfo(utra::perfcounter::RUN_RESET_RUN, 1);
            Err(xous::Error::OutOfMemory)
        }
    }

    #[allow(dead_code)]
    pub fn flush_if_full(&self) -> Result::<(), xous::Error> {
        // check to see if the FIFO is full first. If so, drain it.
        if self.perf_csr.rf(utra::perfcounter::STATUS_FULL) != 0 {
            self.flush()
        } else {
            Ok(())
        }
    }

    /// Flushes any data in the FIFO to the performance buffer. Also stops the performance counter from running.
    /// Returns the total number of entries in the buffer, or an OOM if the buffer is full
    /// This will update the time stamp rollover counter, so, you could in theory restart after this without losing state.
    pub fn stop_and_flush(&self) -> Result::<u32, xous::Error> {
        let mut oom = false;
        // stop the counter
        self.perf_csr.wfo(utra::perfcounter::RUN_STOP, 1);
        // copy over the events to the long term buffer
        let mut ts_offset = 0;
        let mut expected_i = 0;
        while self.perf_csr.rf(utra::perfcounter::STATUS_READABLE) == 1 {
            if *self.log_index.borrow() < self.data.borrow().len() {
                self.dbg_buf_count.replace(self.dbg_buf_count.take() + 1); // this tracks total entries copied into the perfbuf
                self.data.borrow_mut()[*self.log_index.borrow()].code = self.perf_csr.r(utra::perfcounter::EVENT_RAW0);
                ts_offset = self.perf_csr.r(utra::perfcounter::EVENT_RAW1) as u32;
                self.data.borrow_mut()[*self.log_index.borrow()].ts = *self.ts_cont.borrow() as u32 + ts_offset;

                let i = self.perf_csr.r(utra::perfcounter::EVENT_INDEX); // this advances the FIFO
                if i != expected_i {
                    log::info!("i {} != expected_i {}", i, expected_i);
                }
                expected_i += 1;
                self.log_index.replace(self.log_index.take() + 1);
            } else {
                oom = true;
                break;
            }
        }
        self.ts_cont.replace(self.ts_cont.take() + ts_offset as u64);
        log::info!("FIFO final flush had {} entries", expected_i - 1);
        log::info!("Had {} buffer entries", *self.dbg_buf_count.borrow()); // should be the same as above, but trying to chase down some minor issues...
        if !oom {
            Ok(*self.dbg_buf_count.borrow())
        } else {
            Err(xous::Error::OutOfMemory)
        }
    }

    #[allow(dead_code)]
    pub fn print_page_table(&self) {
        log::info!("Buf vmem loc: {:x}", self.data.borrow().as_ptr() as u32);
        match xous::syscall::virt_to_phys(self.data.borrow().as_ptr() as usize) {
            Ok(addr) => log::info!("got {}", addr),
            Err(e) => log::info!("error: {:?}", e),
        };
        log::info!("Buf pmem loc: {:x}", xous::syscall::virt_to_phys(self.data.borrow().as_ptr() as usize).unwrap_or(0));
        log::info!("PerfLogEntry size: {}", core::mem::size_of::<PerfLogEntry>());
        log::info!("Now printing the page table mapping for the performance buffer:");
        for page in (0..BUFLEN).step_by(4096) {
            log::info!("V|P {:x} {:x}",
                self.data.borrow().as_ptr() as usize + page,
                xous::syscall::virt_to_phys(self.data.borrow().as_ptr() as usize + page).unwrap_or(0),
            );
        }
    }
}

pub struct NetCmd {
    callback_id: Option<u32>,
    callback_conn: u32,
    dns: Dns,
    #[cfg(any(target_os = "none", target_os = "xous"))]
    ping: Option<net::Ping>,
    #[cfg(feature="tls")]
    ws: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    #[cfg(feature="perfcounter")]
    perfbuf: xous::MemoryRange,
}
impl NetCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        #[cfg(feature="perfcounter")]
        let perfbuf = xous::syscall::map_memory(
            None,
            None,
            BUFLEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE,
        ).expect("couldn't map in the performance buffer");

        NetCmd {
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
            dns: dns::Dns::new(&xns).unwrap(),
            #[cfg(any(target_os = "none", target_os = "xous"))]
            ping: None,
            #[cfg(feature="tls")]
            ws: None,
            #[cfg(feature="perfcounter")]
            perfbuf,
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCmdDispatch {
    UdpTest1 =  0x1_0000, // we're muxing our own dispatch + ping dispatch, so we need a custom discriminant
    UdpTest2 =  0x1_0001,
}

impl<'a> ShellCmdApi<'a> for NetCmd {
    cmd_api!(net); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        if self.callback_id.is_none() {
            let cb_id = env.register_handler(String::<256>::from_str(self.verb()));
            log::trace!("hooking net callback with ID {}", cb_id);
            self.callback_id = Some(cb_id);
        }

        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        #[cfg(any(target_os = "none", target_os = "xous"))]
        let helpstring = "net [udp [rx socket] [tx dest socket]] [ping [host] [count]] [tcpget host/path]";
        // no ping in hosted mode -- why would you need it? we're using the host's network connection.
        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        let helpstring = "net [udp [port]] [count]] [tcpget host/path]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "unsub" => {
                    // this is just for testing the unsub call itself. It should result in the connection manager itself breaking.
                    match env.netmgr.wifi_state_unsubscribe() {
                        Ok(_) => write!(ret, "wifi unsub successful"),
                        Err(e) => write!(ret, "wifi unsub error: {:?}", e),
                    }.ok();
                }
                "tcpget" => {
                    // note: to keep shellchat lightweight, we do a very minimal parsing of the URL. We assume it always has
                    // a form such as:
                    // bunniefoo.com./bunnie/test.txt
                    // It will break on everything else. The `url` crate is nice but "large" for a demo.
                    // There is no https support, obvs.
                    if let Some(url) = tokens.next() {
                        match url.split_once('/') {
                            Some((host, path)) => {
                                match TcpStream::connect((host, 80)) {
                                    Ok(mut stream) => {
                                        log::trace!("stream open, setting timeouts");
                                        stream.set_read_timeout(Some(Duration::from_millis(10_000))).unwrap();
                                        stream.set_write_timeout(Some(Duration::from_millis(10_000))).unwrap();
                                        log::debug!("read timeout: {:?}", stream.read_timeout().unwrap().unwrap().as_millis());
                                        log::debug!("write timeout: {:?}", stream.write_timeout().unwrap().unwrap().as_millis());
                                        log::info!("my socket: {:?}", stream.local_addr());
                                        log::info!("peer addr: {:?}", stream.peer_addr());
                                        log::info!("sending GET request");
                                        match write!(stream, "GET /{} HTTP/1.1\r\n", path) {
                                            Ok(_) => log::trace!("sent GET"),
                                            Err(e) => {
                                                log::error!("GET err {:?}", e);
                                                write!(ret, "Error sending GET: {:?}", e).unwrap();
                                            }
                                        }
                                        write!(stream, "Host: {}\r\nAccept: */*\r\nUser-Agent: Precursor/0.9.6\r\n", host).expect("stream error");
                                        write!(stream, "Connection: close\r\n").expect("stream error");
                                        write!(stream, "\r\n").expect("stream error");
                                        log::info!("fetching response....");
                                        let mut buf = [0u8; 512];
                                        match stream.read(&mut buf) {
                                            Ok(len) => {
                                                log::trace!("raw response ({}): {:?}", len, &buf[..len]);
                                                write!(ret, "{}", std::string::String::from_utf8_lossy(&buf[..len.min(buf.len())])).ok(); // let it run off the end
                                                log::info!("{}NET.TCPGET,{},{}",
                                                    xous::BOOKEND_START,
                                                    std::string::String::from_utf8_lossy(&buf[..len.min(buf.len())]),
                                                    xous::BOOKEND_END);
                                            }
                                            Err(e) => write!(ret, "Didn't get response from host: {:?}", e).unwrap(),
                                        }
                                    }
                                    Err(e) => write!(ret, "Couldn't connect to {}:80: {:?}", host, e).unwrap(),
                                }
                            }
                            _ => write!(ret, "Usage: tcpget bunniefoo.com/bunnie/test.txt").unwrap(),
                        }
                    } else {
                        write!(ret, "Usage: tcpget bunniefoo.com/bunnie/test.txt").unwrap();
                    }
                }
                "server" => {
                    // this is adapted from https://doc.rust-lang.org/book/ch20-03-graceful-shutdown-and-cleanup.html
                    thread::spawn({
                        let boot_instant = env.boot_instant.clone();
                        move || {
                            let listener = TcpListener::bind("0.0.0.0:80").unwrap();
                            // limit to 4 because we're a bit shy on space in shellchat right now; there is a 32-thread limit per process, and shellchat has the kitchen sink.
                            let pool = ThreadPool::new(4);

                            for stream in listener.incoming() {
                                let stream = match stream {
                                    Ok(s) => s,
                                    Err(e) => {
                                        log::warn!("Listener returned error: {:?}", e);
                                        continue;
                                    }
                                };

                                pool.execute({
                                    let bi = boot_instant.clone();
                                    move || {
                                        handle_connection(stream, bi);
                                    }
                                });
                            }

                            log::info!("demo server shutting down.");
                        }
                    });
                    write!(ret, "TCP listener started on port 80").unwrap();
                    log::info!("{}NET.SERVER,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                }
                "fountain" => {
                    // anything typed after fountain will cause this to be a short test
                    let short_test = tokens.next().is_some();
                    thread::spawn({
                        let short_test = short_test.clone();
                        move || {
                            let tp = threadpool::ThreadPool::new(4);
                            loop {
                                let listener = std::net::TcpListener::bind("0.0.0.0:3333");
                                let listener = match listener {
                                    Ok(listener) => listener,
                                    Err(_) => {
                                        std::thread::sleep(std::time::Duration::from_millis(1000));
                                        continue;
                                    },
                                };

                                for i in listener.incoming() {
                                    match i {
                                        Err(error) => {
                                            log::error!("error caught in listener.incoming(): {}", error);
                                        },
                                        Ok(mut stream) => {
                                            tp.execute(move || {
                                                let mut count = 0;
                                                loop {
                                                    match std::io::Write::write(&mut stream, format!("hello! {}\n", count).as_bytes()) {
                                                        Err(e) => {
                                                            log::info!("fountain write failed with error {:?}", e);
                                                            break;
                                                        }
                                                        _ => {}
                                                    }
                                                    count += 1;
                                                    if count == 10 && short_test {
                                                        stream.flush().ok();
                                                        break;
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    });
                    write!(ret, "Fountain started on port 3333").ok();
                }
                // Testing of udp is done with netcat:
                // to send packets run `netcat -u <precursor ip address> 6502` on a remote host, and then type some data
                // to receive packets, use `netcat -u -l 6502`, on the same remote host, and it should show a packet of counts received
                "udp" => {
                    let socket = if let Some(tok_str) = tokens.next() {
                        tok_str
                    } else {
                        // you could also pass e.g. 127.0.0.1 to check that udp doesn't respond to remote pings, etc.
                        write!(ret, "Usage: net udp 0.0.0.0:6502 [sender_ip:6502], where sender_ip is only necessary if you want the echo-back").unwrap();
                        return Ok(Some(ret));
                    }.to_string();
                    let (response_addr, do_response) = if let Some(r) = tokens.next() {
                        (r.to_string(), true)
                    } else {
                        (std::string::String::new(), false)
                    };
                    use std::net::UdpSocket;
                    let udp = match UdpSocket::bind(socket.clone()) {
                        Ok(udp) => udp,
                        Err(e) => {
                            write!(ret, "Couldn't bind UDP socket: {:?}\n", e).unwrap();
                            return Ok(Some(ret));
                        }
                    };
                    udp.set_write_timeout(Some(Duration::from_millis(500))).expect("couldn't set write timeout");
                    for index in 0..2 {
                        let _ = std::thread::spawn({
                            let self_cid = self.callback_conn;
                            let udp = udp.try_clone().expect("couldn't clone socket");
                            let response = response_addr.clone();
                            move || {
                                const ITERS: usize = 4;
                                let mut iters = 0;
                                let mut s = xous_ipc::String::<512>::new();
                                write!(s, "UDP server {} started", index).unwrap();
                                s.send(self_cid).unwrap();
                                loop {
                                    s.clear();
                                    let mut buf = [0u8; NET_MTU];
                                    match udp.recv_from(&mut buf) {
                                        Ok((bytes, addr)) => {
                                            write!(s, "UDP server {} rx {} bytes: {:?}: {}", index, bytes, addr, std::str::from_utf8(&buf[..bytes]).unwrap()).unwrap();
                                            s.send(self_cid).unwrap();
                                            if do_response {
                                                match udp.send_to(
                                                    format!("Server {} received {} bytes\r\n", index, bytes).as_bytes(),
                                                    &response,
                                                ) {
                                                    Ok(len) => log::info!("server {} sent response of {} bytes", index, len),
                                                    Err(e) => log::info!("server {} UDP tx err: {:?}", index, e),
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("UDP rx failed with {:?}", e);
                                        }
                                    }
                                    iters += 1;
                                    if iters >= ITERS {
                                        break;
                                    }
                                }
                                s.clear();
                                write!(s, "UDP server {} rx closed after {} iters", index, iters).unwrap();
                                s.send(self_cid).unwrap();
                            }
                        });
                    }
                    write!(ret, "Started multi-threaded UDP responder").unwrap();
                }
                "dns" => {
                    if let Some(name) = tokens.next() {
                        match self.dns.lookup(name) {
                            Ok(ipaddr) => {
                                write!(ret, "DNS resolved {}->{:?}", name, ipaddr).unwrap();
                            }
                            Err(e) => {
                                write!(ret, "DNS lookup error: {:?}", e).unwrap();
                            }
                        }
                    }
                }
                #[cfg(feature="nettest")]
                "test" => {
                    crate::nettests::start_batch_tests();
                    write!(ret, "Net batch tests started...").ok();
                }
                #[cfg(feature="ditherpunk")]
                "image" => {
                    if let Some(url) = tokens.next() {
                        match url.split_once('/') {
                            Some((host, path)) => {
                                match TcpStream::connect((host, 80)) {
                                    Ok(mut stream) => {
                                        stream.set_read_timeout(Some(Duration::from_millis(5_000))).unwrap();
                                        stream.set_write_timeout(Some(Duration::from_millis(5_000))).unwrap();
                                        match write!(stream, "GET /{} HTTP/1.1\r\n", path) {
                                            Ok(_) => log::trace!("sent GET"),
                                            Err(e) => {
                                                log::error!("GET err {:?}", e);
                                                write!(ret, "Error sending GET: {:?}", e).unwrap();
                                            }
                                        }
                                        write!(stream, "Host: {}\r\nAccept: */*\r\nUser-Agent: Precursor/0.9.6\r\n", host).expect("stream error");
                                        write!(stream, "Connection: close\r\n").expect("stream error");
                                        write!(stream, "\r\n").expect("stream error");
                                        log::info!("fetching response....");
                                        let mut reader = std::io::BufReader::new(&mut stream);
                                        let mut buf = Vec::<u8>::new();
                                        let mut byte = [0u8; 1];
                                        let mut content_length = 0;
                                        // consume the header - plucking the content-length on the way
                                        const HEADER_LIMIT: usize = 20;
                                        let mut line_count = 0;
                                        while line_count <= HEADER_LIMIT {
                                            line_count += 1;
                                            let mut len = buf.len();
                                            // read a line terminated by /r/n
                                            while (len < 2 || buf.as_slice()[(len-2)..] != [0x0d, 0x0a]) && len <= 1024 {
                                                reader.read(&mut byte).expect("png stream read error");
                                                buf.push(byte[0]);
                                                len = buf.len();
                                            }
                                            match len {
                                                2 => {
                                                   log::info!("found end of header after {} lines.", line_count);
                                                   break;
                                                },
                                                1024.. => {
                                                    let line = std::string::String::from_utf8_lossy(&buf);
                                                    log::warn!("header contained line > 4k {:?}", line);
                                                    break;
                                                }
                                                _ => {},
                                            }
                                            let line = std::string::String::from_utf8_lossy(&buf);
                                            log::info!("{:?}", line);
                                            let l_line_split = line.to_ascii_lowercase();
                                            let l_line: Vec<&str> = l_line_split.split(':').collect();
                                            if l_line.len() > 1 {
                                                log::info!("attr: {}, {}", l_line[0], l_line[1]);
                                                match l_line[0] {
                                                    "content-length" => {
                                                        content_length = usize::from_str(l_line[1].trim()).unwrap_or(0);
                                                        log::info!("found content-length of {}", content_length);
                                                    }
                                                    _ => {}
                                                }
                                            };
                                            buf.clear();
                                        }

                                        if content_length > 0 {
                                            log::info!("heap size: {}", heap_usage());
                                            let mut png = DecodePng::new(reader).expect("png decode failed");
                                            const BORDER: u32 = 3;
                                            let modal_size = gam::Point::new(
                                                 (gam::IMG_MODAL_WIDTH - 2 * BORDER) as i16,
                                                 (gam::IMG_MODAL_HEIGHT - 2 * BORDER) as i16
                                            );
                                            let bm = gam::Bitmap::from_png(&mut png, Some(modal_size));

                                            log::info!("heap size: {}", heap_usage());
                                            let modals = modals::Modals::new(&env.xns).unwrap();
                                            modals.show_image(bm).expect("show image modal failed");

                                        } else {
                                            write!(ret, "content-length was 0, no image read").unwrap();
                                        }
                                    }
                                    Err(e) => write!(ret, "Couldn't connect to {}:80: {:?}", host, e).unwrap(),
                                }
                            }
                            _ => write!(ret, "Usage: image bunniefoo.com/bunnie/bunny.png").unwrap(),
                        }
                    } else {
                        write!(ret, "Usage: image bunniefoo.com/bunnie/bunny.png").unwrap();
                    }
                }
                // only valid for hardware configs with TLS enabled
                #[cfg(all(any(target_os = "none", target_os = "xous"),feature="tls"))]
                "rt" => {
                    log::set_max_level(log::LevelFilter::Trace);
                    ring::xous_test::p256_elem_add_test();
                    log::set_max_level(log::LevelFilter::Info);
                }
                #[cfg(feature="tls")]
                "ws" => {
                    if self.ws.is_none() {
                        let (socket, response) =
                        tungstenite::connect(url::Url::parse("wss://awake.noskills.club/ws").unwrap()).expect("Can't connect");

                        log::info!("Connected to the server");
                        log::info!("Response HTTP code: {}", response.status());
                        log::info!("Response contains the following headers:");
                        for (ref header, _value) in response.headers() {
                            log::info!("* {}", header);
                        }
                        self.ws = Some(socket);
                    }
                    let mut err = false;
                    if let Some(socket) = &mut self.ws {
                        let mut val = String::<1024>::new();
                        join_tokens(&mut val, &mut tokens);
                        if val.len() > 0 {
                            socket.write_message(tungstenite::Message::Text(val.as_str().unwrap().into())).unwrap();
                        } else {
                            socket.write_message(tungstenite::Message::Text("Hello WebSocket".into())).unwrap();
                        }
                        match socket.read_message() {
                            Ok(msg) => {
                                log::info!("Received: {}", msg);
                                write!(ret, "Rx: {}", msg).ok();
                            },
                            Err(e) => {
                                log::info!("got ws error: {:?}, quitting", e);
                                err = true;
                                socket.close(None).ok();
                            }
                        }
                    }
                    if err {
                        self.ws.take();
                        write!(ret, "\nWeb socket session closed.").ok();
                    }
                }
                #[cfg(feature="tls")]
                "tls" => {
                    log::set_max_level(log::LevelFilter::Info);
                    log::info!("starting TLS run");
                    let mut root_store = rustls::RootCertStore::empty();
                    log::info!("create root store");
                    root_store.add_server_trust_anchors(
                        webpki_roots::TLS_SERVER_ROOTS
                            .0
                            .iter()
                            .map(|ta| {
                                rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                                    ta.subject,
                                    ta.spki,
                                    ta.name_constraints,
                                )
                            })
                    );
                    log::info!("build TLS client config");
                    let config = rustls::ClientConfig::builder()
                        .with_safe_defaults()
                        .with_root_certificates(root_store)
                        .with_no_client_auth();

                    log::info!("point TLS to bunniefoo.com");
                    let server_name = "bunniefoo.com".try_into().unwrap();
                    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();

                    log::info!("connect TCPstream to bunniefoo.com");
                    let mut sock = TcpStream::connect("bunniefoo.com:443").unwrap();
                    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
                    log::info!("create http headers and write to server");
                    tls.write_all(
                        concat!(
                            "GET / HTTP/1.1\r\n",
                            "Host: bunniefoo.com\r\n",
                            "Connection: close\r\n",
                            "Accept-Encoding: identity\r\n",
                            "\r\n"
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                    log::info!("readout cipher suite");
                    let ciphersuite = tls
                        .conn
                        .negotiated_cipher_suite()
                        .unwrap();
                    log::info!(
                        "Current ciphersuite: {:?}",
                        ciphersuite.suite()
                    );
                    let mut plaintext = Vec::new();
                    log::info!("read TLS response");
                    tls.read_to_end(&mut plaintext).unwrap();
                    log::info!("len: {}", plaintext.len());
                    log::info!("{}", std::str::from_utf8(&plaintext).unwrap_or("utf-error"));
                    log::set_max_level(log::LevelFilter::Info);
                }
                #[cfg(feature="perfcounter")]
                "v2p" => {
                    // don't generate a new object since it clears the data, just do a raw dump
                    log::info!("Buf vmem loc: {:x}", self.perfbuf.as_ptr() as u32);
                    log::info!("Buf pmem loc: {:x}", xous::syscall::virt_to_phys(self.perfbuf.as_ptr() as usize).unwrap_or(0));
                    log::info!("PerfLogEntry size: {}", core::mem::size_of::<PerfLogEntry>());
                    log::info!("Now printing the page table mapping for the performance buffer:");
                    for page in (0..BUFLEN).step_by(4096) {
                        log::info!("V|P {:x} {:x}",
                            self.perfbuf.as_ptr() as usize + page,
                            xous::syscall::virt_to_phys(self.perfbuf.as_ptr() as usize + page).unwrap_or(0),
                        );
                    }
                }
                #[cfg(feature="perfcounter")]
                "cta1" => {
                    log::info!("constant time RISC-V HW AES test");

                    let pm = PerfMgr::new(
                        self.perfbuf.as_mut_ptr(),
                        env.perf_csr.clone(),
                        env.event_csr.clone()
                    );
                    pm.stop_and_reset();

                    // this starts the performance counter
                    pm.start();

                    use aes::Aes256;
                    use aes::cipher::{BlockEncrypt, KeyInit};
                    use aes::cipher::generic_array::GenericArray;

                    let mut key_array: [u8; 32];
                    let mut data_array: [u8; 16];
                    for databit in 0..128 {
                        data_array = [0; 16];
                        data_array[databit / 8] = 1 << databit % 8;
                        for keybit in 0..256 {
                            key_array = [0; 32];
                            key_array[keybit / 8] = 1 << keybit % 8;
                            let cipher_hw = Aes256::new(&key_array.into());
                            let mut block = GenericArray::clone_from_slice(&mut data_array);

                            // demarcate the encryption operation performance counter events
                            pm.log_event_unchecked(((databit as u32) << 8) | keybit as u32);
                            cipher_hw.encrypt_block(&mut block);
                            pm.log_event_unchecked(0x100_0000 | ((databit as u32) << 8) | keybit as u32);
                        }
                        if databit % 4 == 3 {
                            pm.flush().ok();
                        }
                    }
                    match pm.stop_and_flush() {
                        Ok(entries) => {
                            log::info!("entries: {}", entries);
                        }
                        _ => {
                            log::info!("Perfcounter OOM'd during run");
                        }
                    }
                    // pm.print_page_table();
                }
                #[cfg(feature="perfcounter")]
                "cta2" => {
                    let pm = PerfMgr::new(
                        self.perfbuf.as_mut_ptr(),
                        env.perf_csr.clone(),
                        env.event_csr.clone()
                    );
                    pm.stop_and_reset();

                    // this starts the performance counter
                    pm.start();

                    use ring::xous_test::aes_nohw::aes_key_st;

                    let mut key_array: [u8; 32];
                    let mut data_array: [u8; 16];
                    let mut out_array: [u8; 16] = [0u8; 16];
                    for databit in 0..128 {
                        data_array = [0; 16];
                        data_array[databit / 8] = 1 << databit % 8;
                        for keybit in 0..256 {
                            key_array = [0; 32];
                            key_array[keybit / 8] = 1 << keybit % 8;
                            let mut schedule = aes_key_st {
                                rd_key: [0u32; 60],
                                rounds: 0
                            };
                            ring::xous_test::expand_aes_key(&key_array, &mut schedule);
                            // demarcate the encryption operation performance counter events
                            pm.log_event_unchecked(((databit as u32) << 8) | keybit as u32);
                            ring::xous_test::aes_encrypt(&data_array, &mut out_array, &schedule);
                            pm.log_event_unchecked(0x100_0000 | ((databit as u32) << 8) | keybit as u32);
                        }
                        if databit % 4 == 3 {
                            pm.flush().ok();
                        }
                    }
                    match pm.stop_and_flush() {
                        Ok(entries) => {
                            log::info!("entries: {}", entries);
                        }
                        _ => {
                            log::info!("Perfcounter OOM'd during run");
                        }
                    }
                }
                #[cfg(any(target_os = "none", target_os = "xous"))]
                "ping" => {
                    if let Some(name) = tokens.next() {
                        match self.dns.lookup(name) {
                            Ok(ipaddr) => {
                                log::debug!("sending ping to {:?}", ipaddr);
                                if self.ping.is_none() {
                                    self.ping = Some(net::Ping::non_blocking_handle(
                                        XousServerId::ServerName(xous_ipc::String::from_str(crate::SERVER_NAME_SHELLCHAT)),
                                        self.callback_id.unwrap() as usize,
                                    ));
                                }
                                if let Some(count_str) = tokens.next() {
                                    let count = count_str.parse::<u32>().unwrap();
                                    if let Some(pinger) = &self.ping {
                                        pinger.ping_spawn_thread(
                                            IpAddr::from(ipaddr),
                                            count as usize,
                                            1000
                                        );
                                        write!(ret, "Sending {} pings to {} ({:?})", count, name, ipaddr).unwrap();
                                    } else {
                                        // this just shouldn't happen based on the structure of the code above.
                                        write!(ret, "Can't ping, internal error.").unwrap();
                                    }
                                } else {
                                    if let Some(pinger) = &self.ping {
                                        if pinger.ping(IpAddr::from(ipaddr)) {
                                            write!(ret, "Sending a ping to {} ({:?})", name, ipaddr).unwrap();
                                        } else {
                                            write!(ret, "Couldn't send a ping to {}, maybe socket is busy?", name).unwrap();
                                        }
                                    } else {
                                        write!(ret, "Can't ping, internal error.").unwrap();
                                    }
                                };
                            }
                            Err(e) => {
                                write!(ret, "Can't ping, DNS lookup error: {:?}", e).unwrap();
                            }
                        }
                    } else {
                        write!(ret, "Missing host: net ping [host] [count]").unwrap();
                    }
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }

        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }


    fn callback(&mut self, msg: &MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        log::debug!("net callback");
        let mut ret = String::<1024>::new();
        match &msg.body {
            xous::Message::Scalar(xous::ScalarMessage {id: _, arg1, arg2, arg3, arg4}) => {
                let dispatch = *arg1;
                match FromPrimitive::from_usize(dispatch) {
                    Some(NetCmdDispatch::UdpTest1) => {
                        // Not used after udp to libstd, but left in case we want to repurpose
                    },
                    Some(NetCmdDispatch::UdpTest2) => {
                        // Not used after udp to libstd
                    },
                    None => {
                        // rebind the scalar args to the Ping convention
                        let op = arg1;
                        let addr = IpAddr::from((*arg2 as u32).to_be_bytes());
                        let seq_or_addr = *arg3;
                        let timestamp = *arg4;
                        match FromPrimitive::from_usize(op & 0xFF) {
                            Some(NetPingCallback::Drop) => {
                                // write!(ret, "Info: All pending pings done").unwrap();
                                // ignore the message, just creates visual noise
                                return Ok(None);
                            }
                            Some(NetPingCallback::NoErr) => {
                                match addr {
                                    IpAddr::V4(_) => {
                                        write!(ret, "Pong from {:?} seq {} received: {} ms",
                                        addr,
                                        seq_or_addr,
                                        timestamp).unwrap();
                                        log::info!("{}NET.PONG,{:?},{},{},{}",
                                            xous::BOOKEND_START,
                                            addr,
                                            seq_or_addr,
                                            timestamp,
                                            xous::BOOKEND_END
                                        );
                                    },
                                    IpAddr::V6(_) => {
                                        write!(ret, "Ipv6 pong received: {} ms", timestamp).unwrap();
                                    },
                                }
                            }
                            Some(NetPingCallback::Timeout) => {
                                write!(ret, "Ping to {:?} timed out", addr).unwrap();
                            }
                            Some(NetPingCallback::Unreachable) => {
                                let code = net::Icmpv4DstUnreachable::from((op >> 24) as u8);
                                write!(ret, "Ping to {:?} unreachable: {:?}", addr, code).unwrap();
                            }
                            None => {
                                log::error!("Unknown opcode received in NetCmd callback: {:?}", op);
                                write!(ret, "Unknown opcode received in NetCmd callback: {:?}", op).unwrap();
                            }
                        }
                    },
                }
            },
            xous::Message::Move(m) => {
                let s = xous_ipc::String::<512>::from_message(m).unwrap();
                write!(ret, "{}", s.as_str().unwrap()).unwrap();
            }
            _ => {
                log::error!("got unecognized message type in callback handler")
            }
        }
        Ok(Some(ret))
    }
}

enum Responses {
    Uptime,
    NotFound,
    Buzz,
}

fn handle_connection(mut stream: TcpStream, boot_instant: Instant) {
    // the result is implementation dependent, on Xous hardware, this is effectively the same as ticktimer.elapsed_ms()
    let elapsed_time = Instant::now().duration_since(boot_instant);
    let uptime = std::format!("Hello from Precursor!\n\rI have been up for {}:{:02}:{:02}.\n\r",
        (elapsed_time.as_millis() / 3_600_000),
        (elapsed_time.as_millis() / 60_000) % 60,
        (elapsed_time.as_millis() / 1000) % 60,
    );

    let mut buffer = [0; 1024];
    match stream.read(&mut buffer) {
        Ok(_) => {},
        Err(e) => {
            log::warn!("Server connection error; closing connection {:?}", e);
            return;
        }
    }

    let get = b"GET / HTTP/1.1\r\n";
    let sleep = b"GET /sleep HTTP/1.1\r\n";
    let buzz = b"GET /buzz HTTP/1.1\r\n";

    let (status_line, response_index) = if buffer.starts_with(get) {
        ("HTTP/1.1 200 OK", Responses::Uptime)
    } else if buffer.starts_with(sleep) {
        thread::sleep(Duration::from_secs(5));
        ("HTTP/1.1 200 OK", Responses::Uptime)
    } else if buffer.starts_with(buzz) {
        ("HTTP/1.1 200 OK", Responses::Buzz)
    } else {
        ("HTTP/1.1 404 NOT FOUND", Responses::NotFound)
    };

    let contents = match response_index {
        Responses::Uptime => {
            uptime.as_str()
        },
        Responses::Buzz => {
            let xns = xous_names::XousNames::new().unwrap();
            llio::Llio::new(&xns).vibe(llio::VibePattern::Double).ok();
            "The motor on the Precursor goes bzz bzz"
        }
        Responses::NotFound => {
            "Ceci n'est pas une page vide"
        },
    };

    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );

    stream.write(response.as_bytes()).ok();
    stream.flush().ok();
}

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

enum Message {
    NewJob(Job),
    Terminate,
}

impl ThreadPool {
    /// Create a new ThreadPool.
    ///
    /// The size is the number of threads in the pool.
    ///
    /// # Panics
    ///
    /// The `new` function will panic if the size is zero.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();

        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(Message::NewJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        log::info!("Sending terminate message to all workers.");

        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        log::info!("Shutting down all workers.");

        for worker in &mut self.workers {
            log::info!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();

            match message {
                Message::NewJob(job) => {
                    log::debug!("Worker {} got a job; executing.", id);

                    job();
                }
                Message::Terminate => {
                    log::info!("Worker {} was told to terminate.", id);

                    break;
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

#[cfg(feature="ditherpunk")]
fn heap_usage() -> usize {
    match xous::rsyscall(xous::SysCall::IncreaseHeap(0, xous::MemoryFlags::R)).expect("couldn't get heap size") {
        xous::Result::MemoryRange(m) => {
            let usage = m.len();
            usage
        }
        _ => {
            log::error!("Couldn't measure heap usage");
            0
         },
    }
}

#[cfg(feature ="tls")]
fn join_tokens<'a>(buf: &mut String<1024>, tokens: impl Iterator<Item = &'a str>) {
    use core::fmt::Write;
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
