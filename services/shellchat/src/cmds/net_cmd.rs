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

pub struct NetCmd {
    callback_id: Option<u32>,
    callback_conn: u32,
    dns: Dns,
    #[cfg(any(target_os = "none", target_os = "xous"))]
    ping: Option<net::Ping>,
}
impl NetCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        NetCmd {
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
            dns: dns::Dns::new(&xns).unwrap(),
            #[cfg(any(target_os = "none", target_os = "xous"))]
            ping: None,
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
                    };
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
                                                write!(ret, "{}", std::string::String::from_utf8_lossy(&buf[..len])).ok(); // let it run off the end
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
                            // limit to 2 because we're a bit shy on space in shellchat right now; there is a 32-thread limit per process, and shellchat has the kitchen sink.
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
                                        log::info!("connection closed");
                                    }
                                });
                            }

                            log::info!("demo server shutting down.");
                        }
                    });
                    write!(ret, "TCP listener started on port 80").unwrap();
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
                "tls" => {

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
    stream.read(&mut buffer).unwrap();

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

    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
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
                    log::info!("Worker {} got a job; executing.", id);

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