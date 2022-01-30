use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::unimplemented;
use std::io;
use std::io::{Error, ErrorKind, Result};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use smoltcp::time::Duration;

use xous::{CID, Message, SID, msg_blocking_scalar_unpack};
use xous_ipc::Buffer;
use crate::NetConn;
use crate::api::*;
//use crate::api::udp::*;
use num_traits::*;

use std::net::Shutdown;
use std::fmt;
use std::collections::VecDeque;

// These constants trade off power-savings versus latency when TCP connections block.
const RX_POLL_INTERVAL_MS: usize = 20;
const TX_POLL_INTERVAL_MS: usize = 20;

// used by both clients and servers. implement this first.
pub struct TcpStream {
    net: NetConn,
    cb_sid: SID,
    socket_addr: SocketAddr,
    local_port: u16,
    ticktimer: ticktimer_server::Ticktimer,

    rx_buf: Arc::<Mutex::<VecDeque::<u8>>>,
    /// xous-specific feature that allows for more efficent Rx than blocking. Caller must set_scalar_notification() to use it.
    rx_notify: Arc<Mutex<XousScalarEndpoint>>,
    rx_refcount: Arc<AtomicU32>,

    nonblocking: bool,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    rx_shutdown: bool,
    tx_shutdown: bool,
}

impl TcpStream {
    pub fn connect(maybe_socket: io::Result<&SocketAddr>) -> io::Result<TcpStream> {
        TcpStream::connect_inner(maybe_socket, None, None)
    }
    pub fn connect_timeout(maybe_socket: io::Result<&SocketAddr>, duration: Duration) -> io::Result<TcpStream> {
        TcpStream::connect_inner(maybe_socket, Some(duration), None)
    }

    /// This API call uses the stdlib-internal calling convention to assist with the process of migrating this into a true libstd
    pub fn connect_xous<A: ToSocketAddrs>(addr: A, timeout: Option<Duration>, keepalive: Option<Duration>) -> Result<TcpStream> {
        match addr.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        TcpStream::connect_inner(
                            Ok(&socket_addr),
                            timeout,
                            keepalive
                        )
                    }
                    _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
                }
            }
            _ => Err(Error::new(ErrorKind::InvalidInput, "IP address invalid"))
        }
    }
    /// note that there is no provision in the rust stdlib to set a keepalive, so it's sort of wedged in here
    pub fn connect_inner(
        maybe_socket: io::Result<&SocketAddr>,
        timeout: Option<Duration>,
        keepalive: Option<Duration>) -> io::Result<TcpStream> {
        let socket = maybe_socket?;
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();
        let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

        let rx_buf = Arc::new(Mutex::new(VecDeque::<u8>::new()));
        rx_buf.lock().unwrap().reserve(TCP_BUFFER_SIZE);
        let notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));

        let _handle = thread::spawn({
            let cb_sid_clone = cb_sid.clone();
            let rx_buf = Arc::clone(&rx_buf);
            let notify = Arc::clone(&notify);
            move || {
                loop {
                    let msg = xous::receive_message(cb_sid_clone).unwrap();
                    match FromPrimitive::from_usize(msg.body.id()) {
                        Some(NetTcpCallback::RxData) => {
                            let buffer = unsafe {Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                            let incoming = buffer.as_flat::<NetTcpResponse, _>().unwrap();
                            { // grab the lock once for better efficiency
                                let mut rx_locked = rx_buf.lock().unwrap();
                                for &d in incoming.data[..incoming.len as usize].iter() {
                                    rx_locked.push_back(d);
                                }
                            }
                            notify.lock().unwrap().notify(); // this will only notify if a destination has been set
                        }
                        Some(NetTcpCallback::Drop) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                            log::debug!("Drop received, exiting Tcp handler");
                            xous::return_scalar(msg.sender, 1).unwrap();
                            break;
                        }),
                        None => {
                            log::error!("got unknown message type on Tcp callback: {:?}", msg);
                        }
                    }
                }
            }
        });

        let request = NetTcpManage {
            cb_sid: cb_sid.to_array(),
            ip_addr: NetIpAddr::from(socket.ip()),
            remote_port: socket.port(),
            local_port: None,
            timeout_ms: if let Some(d) = timeout { Some(d.total_millis()) } else { None },
            keepalive_ms: if let Some(d) = keepalive { Some(d.total_millis()) } else { None },
            result: None,
            mgmt_code: None,
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::TcpConnect.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't register with Net server")))?;
        let ret = buf.to_original::<NetTcpManage, _>().unwrap();
        match ret.result {
            Some(NetMemResponse::Ok) => {
                if let Some(local_port) = ret.local_port {
                    Ok(TcpStream {
                        net,
                        cb_sid,
                        socket_addr: *socket,
                        local_port,
                        rx_buf,
                        rx_notify: notify,
                        read_timeout: None,
                        write_timeout: None,
                        ticktimer,
                        nonblocking: false,
                        rx_shutdown: false,
                        tx_shutdown: false,
                        rx_refcount: Arc::new(AtomicU32::new(1)), // one instance, that's me!
                    })
                } else {
                    Err(Error::new(ErrorKind::Other, "Net server failed to assign us a local port"))
                }
            },
            _ => {
                Err(Error::new(ErrorKind::Other, "can't register with Net server"))
            }
        }
    }
    pub fn set_scalar_notification(&mut self, cid: CID, op: usize, args: [Option<usize>; 4]) {
        self.rx_notify.lock().unwrap().set(cid, op, args);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.rx_notify.lock().unwrap().clear();
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if let Some(duration) = timeout {
            if duration.total_millis() == 0 {
                Err(Error::new(ErrorKind::InvalidInput, "zero duration is not valid"))
            } else {
                self.read_timeout = Some(duration);
                Ok(())
            }
        } else {
            self.read_timeout = None;
            Ok(())
        }
    }
    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        if let Some(duration) = timeout {
            if duration.total_millis() == 0 {
                Err(Error::new(ErrorKind::InvalidInput, "zero duration is not valid"))
            } else {
                self.write_timeout = Some(duration);
                Ok(())
            }
        } else {
            self.write_timeout = None;
            Ok(())
        }
    }
    pub fn read_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.read_timeout)
    }
    pub fn write_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(self.write_timeout)
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.socket_addr)
    }
    pub fn socket_addr(&self) -> io::Result<SocketAddr> {
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            self.local_port,
        ))
    }

    fn tcp_manage(&self, code: TcpMgmtCode) -> io::Result<TcpMgmtCode> {
        let request = NetTcpManage {
            cb_sid: self.cb_sid.to_array(),
            ip_addr: NetIpAddr::from(self.socket_addr),
            remote_port: self.socket_addr.port(),
            local_port: Some(self.local_port),
            timeout_ms: None,
            keepalive_ms: None,
            result: None,
            mgmt_code: Some(code),
        };
        let mut buf = Buffer::into_buf(request)
            .or(Err(Error::new(ErrorKind::Other, "can't manage TCP")))?;
        buf.lend_mut(self.net.conn(), Opcode::TcpManage.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't manage TCP")))?;
        let ret = buf.to_original::<NetTcpManage, _>().unwrap();
        if let Some(code) = ret.mgmt_code {
            Ok(code)
        } else {
            Err(Error::new(ErrorKind::Other, "Internal error"))
        }
    }

    pub fn shutdown(&mut self, request: Shutdown) -> io::Result<()> {
        match request {
            Shutdown::Read | Shutdown::Both => {
                if request == Shutdown::Both {
                    self.tx_shutdown = true;
                }
                match self.tcp_manage(TcpMgmtCode::SetRxShutdown) {
                    Ok(_) => {
                        self.rx_shutdown = true;
                        Ok(())
                    }
                    Err(_) => {
                        Err(Error::new(ErrorKind::Other, "Internal error"))
                    }
                }
            }
            Shutdown::Write => {
                self.tx_shutdown = true;
                Ok(())
            }
        }
    }

    pub fn set_nodelay(&self, setting: bool) -> io::Result<()> {
        self.tcp_manage(TcpMgmtCode::SetNoDelay(setting)).map(|_| ())
    }

    pub fn nodelay(&self) -> io::Result<bool> {
        match self.tcp_manage(TcpMgmtCode::GetNoDelay(false)) {
            Ok(TcpMgmtCode::GetNoDelay(value)) => Ok(value),
            _ => Err(Error::new(ErrorKind::Other, "Internal error")),
        }
    }

    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.tcp_manage(TcpMgmtCode::SetTtl(ttl)).map(|_| ())
    }

    pub fn ttl(&self) -> io::Result<u32> {
        match self.tcp_manage(TcpMgmtCode::GetTtl(0)) {
            Ok(TcpMgmtCode::GetTtl(value)) => Ok(value),
            _ => Err(Error::new(ErrorKind::Other, "Internal error")),
        }
    }

    /// I'm not quite sure this has any meaning on Xous. Right now this just checks if there is an
    /// Rx state error in progress.
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        match self.tcp_manage(TcpMgmtCode::ErrorCheck(NetMemResponse::LibraryError)) {
            Ok(TcpMgmtCode::ErrorCheck(value)) => {
                match value {
                    NetMemResponse::Finished =>
                        Ok(Some(io::Error::new(ErrorKind::ConnectionAborted, "TCP Fast Open not supported"))),
                    NetMemResponse::Invalid =>
                        Ok(Some(io::Error::new(ErrorKind::OutOfMemory, "Cant' receive"))),
                    NetMemResponse::Ok => Ok(None),
                    _ => Ok(Some(io::Error::new(ErrorKind::Other, "Call failed"))),
                }
            },
            _ => Err(Error::new(ErrorKind::Other, "Internal error")),
        }
    }

    pub fn set_nonblocking(&mut self, setting: bool) -> io::Result<()> {
        self.nonblocking = setting;
        Ok(())
    }

    pub fn duplicate(&self) -> io::Result<TcpStream> {
        let xns = xous_names::XousNames::new().unwrap();
        let cloned_stream = TcpStream {
            net: NetConn::new(&xns).unwrap(),
            cb_sid: self.cb_sid,
            socket_addr: self.socket_addr,
            local_port: self.local_port,
            ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
            rx_buf: self.rx_buf.clone(),
            rx_notify: self.rx_notify.clone(),
            nonblocking: self.nonblocking,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            rx_shutdown: self.rx_shutdown,
            tx_shutdown: self.tx_shutdown,
            rx_refcount: self.rx_refcount.clone(),
        };
        self.rx_refcount.fetch_add(1, Ordering::SeqCst);
        Ok(cloned_stream)
    }

    pub fn peek(&self, _: &mut [u8]) -> io::Result<usize> {
        unimplemented!()
    }
}

impl fmt::Debug for TcpStream {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unimplemented!("work in progress")
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.nonblocking {
            if self.rx_buf.lock().unwrap().len() == 0 {
                return Err(Error::new(ErrorKind::WouldBlock, "Read would block"));
            } else {
                let mut rx_buf = self.rx_buf.lock().unwrap();
                let readlen = rx_buf.len().min(buf.len());
                for (src, dst) in rx_buf.drain(..readlen).zip(buf.iter_mut()) {
                    *dst = src;
                }
                return Ok(readlen)
            }
        } else {
            let start = self.ticktimer.elapsed_ms();
            while self.ticktimer.elapsed_ms() - start <
                self.read_timeout.unwrap_or(Duration::from_millis(u64::MAX)).millis() {
                if self.rx_buf.lock().unwrap().len() == 0 {
                    // this limits our poll interval frequency. We do this mainly to save power --
                    // we certainly could do a xous::yield_slice(); which would cause this to immediately
                    // and furiously return if the system is busy-waiting, but this will also increase
                    // battery usage by quite a bit.
                    self.ticktimer.sleep_ms(RX_POLL_INTERVAL_MS).unwrap();
                } else {
                    let mut rx_buf = self.rx_buf.lock().unwrap();
                    let readlen = rx_buf.len().min(buf.len());
                    for (src, dst) in rx_buf.drain(..readlen).zip(buf.iter_mut()) {
                        *dst = src;
                    }
                    return Ok(readlen)
                }
            }
            // the return code in this case varies by platform. We're going with what the docs say is the "Unix" way.
            return Err(Error::new(ErrorKind::WouldBlock, "Read timed out"));
        }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut tx_chunk = NetTcpTransmit {
            remote_addr: NetIpAddr::from(self.socket_addr),
            remote_port: self.socket_addr.port(),
            local_port: self.local_port,
            len: 0,
            data: [0u8; TCP_BUFFER_SIZE],
            result: None,
        };
        // send data to the Net block in native buffer-sized chunks, because in practice, this is all you could
        // process anyways in one go...
        let mut total_tx = 0usize;
        let start = self.ticktimer.elapsed_ms();
        let mut abort = false;
        for mtu in buf.chunks(TCP_BUFFER_SIZE) {
            for (&src, dst) in mtu.iter().zip(tx_chunk.data.iter_mut()) {
                *dst = src;
            }
            tx_chunk.len = mtu.len() as u16;
            tx_chunk.result = None;
            loop {
                let mut buf = Buffer::into_buf(tx_chunk)
                    .or(Err(Error::new(ErrorKind::Other, "internal error handling Tx")))?;
                buf.lend_mut(self.net.conn(), Opcode::TcpTx.to_u32().unwrap())
                    .or(Err(Error::new(ErrorKind::Other, "internal error handling Tx")))?;
                let ret = buf.as_flat::<NetTcpTransmit, _>().unwrap();
                match ret.result {
                    rkyv::core_impl::ArchivedOption::Some(ArchivedNetMemResponse::Sent(txlen)) => {
                        total_tx += txlen as usize;
                        if txlen > 0 {
                            break;
                        }
                        if txlen == 0 && total_tx != 0 {
                            // we set *something* -- in this case,
                            // we can just return Ok(total_tx) and this is "not an error"
                            // note that this condition only triggers if the first packet sent, and a later packet turned out to be busy.
                            abort = true;
                            break;
                        }
                        if total_tx == 0 && txlen == 0 {
                            if self.nonblocking {
                                return Err(Error::new(ErrorKind::WouldBlock, "Write would block, and nonblocking is set"));
                            } else {
                                if let Some(d) = self.write_timeout {
                                    if self.ticktimer.elapsed_ms() - start > d.millis() {
                                        return Err(Error::new(ErrorKind::WouldBlock, "Write would block, and nonblocking is set"));
                                    }
                                }
                                self.ticktimer.sleep_ms(TX_POLL_INTERVAL_MS).unwrap();
                            }
                        }
                    }
                    _ => return Err(Error::new(ErrorKind::Other, "internal error handling Tx")),
                }
            }
            if abort {
                break;
            }
        }
        assert!(total_tx <= buf.len(), "Tx length inconsistency error");
        Ok(total_tx)
    }

    /// In the case that Nagle's algo is enabled, packets can get stuck in the outgoing buffer.
    /// This basically waits until Nagle's algo times out, and the data is sent.
    fn flush(&mut self) -> io::Result<()> {
        loop {
            match self.tcp_manage(TcpMgmtCode::Flush(false)) {
                Ok(TcpMgmtCode::Flush(flushed)) => {
                    if flushed {
                        return Ok(())
                    }
                }
                _ => return Err(Error::new(ErrorKind::Other, "internal error handling flush"))
            }
            self.ticktimer.sleep_ms(TX_POLL_INTERVAL_MS).unwrap();
        }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        if self.rx_refcount.fetch_sub(1, Ordering::SeqCst) == 1 {
            let request = NetTcpManage {
                ip_addr: NetIpAddr::from(self.socket_addr),
                remote_port: self.socket_addr.port(),
                local_port: Some(self.local_port),
                cb_sid: self.cb_sid.to_array(),
                timeout_ms: None,
                keepalive_ms: None,
                result: None,
                mgmt_code: None,
            };
            let mut buf = Buffer::into_buf(request).expect("can't unregister with Net server");
            buf.lend_mut(self.net.conn(), Opcode::TcpClose.to_u32().unwrap()).expect("can't unregister with Net server");
            match buf.to_original().unwrap() {
                NetMemResponse::Ok => {
                    let drop_cid = xous::connect(self.cb_sid).unwrap();
                    xous::send_message(
                        drop_cid,
                        Message::new_blocking_scalar(NetTcpCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
                    ).expect("couldn't send Drop to our repsonding server");
                    unsafe{xous::disconnect(drop_cid).ok()}; // should be safe because we're the only connection and the previous was a blocking scalar
                },
                _ => {
                    panic!("Couldn't unregister with net server");
                }
            }
            // now we can detroy the server id of the responder thread
            xous::destroy_server(self.cb_sid).ok();
        }
    }
}