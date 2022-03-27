use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
#[cfg(any(target_os = "none", target_os = "xous"))]
use net::XousServerId;
use net::{Duration, NetPingCallback};
use xous::MessageEnvelope;
use num_traits::*;
use std::net::{SocketAddr, IpAddr, TcpStream};
use std::io::Write;
use std::io::Read;
use dns::Dns; // necessary to work around https://github.com/rust-lang/rust/issues/94182

pub struct NetCmd {
    udp: Option<net::UdpSocket>,
    udp_clone: Option<net::UdpSocket>,
    callback_id: Option<u32>,
    callback_conn: u32,
    udp_count: u32,
    dns: Dns,
    #[cfg(any(target_os = "none", target_os = "xous"))]
    ping: Option<net::Ping>,
}
impl NetCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        NetCmd {
            udp: None,
            udp_clone: None,
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
            udp_count: 0,
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

pub const UDP_TEST_SIZE: usize = 64;
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
        let helpstring = "net [udp [port]] [udpclose] [udpclone] [udpcloneclose] [ping [host] [count]] [tcpget host/path]";
        // no ping in hosted mode -- why would you need it? we're using the host's network connection.
        #[cfg(not(any(target_os = "none", target_os = "xous")))]
        let helpstring = "net [udp [port]] [udpclose] [udpclone] [udpcloneclose] [count]] [tcpget host/path]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
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
                                        stream.set_read_timeout(Some(std::time::Duration::from_millis(10_000))).unwrap();
                                        stream.set_write_timeout(Some(std::time::Duration::from_millis(10_000))).unwrap();
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
                                                write!(ret, "{}", std::string::String::from_utf8_lossy(&buf[..len])).unwrap();
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
                    // PLEASE NOTE:
                    // Trying to make a TCP server of some kind? Don't be shy to open an issue at
                    // https://github.com/betrusted-io/xous-core/issues. The TCP stack is very thinly
                    // tested. Also, this is not a "real" web server, obviously -- so it's going to have quirks,
                    // such as Firefox reporting that connections have been reset because this doesn't implement
                    // a complete HTTP life cycle.
                    let _ = std::thread::spawn({
                        let mut listener = net::TcpListener::bind_xous(
                            "127.0.0.1:80"
                        ).unwrap();
                        let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
                        let callback_conn = self.callback_conn.clone();
                        move || {
                            loop {
                                match listener.accept() {
                                    Ok((mut stream, addr)) => {
                                        let elapsed_time = ticktimer.elapsed_ms();
                                        let test_string = std::format!("Hello from Precursor!\n\rI have been up for {}:{:02}:{:02}.\n\r",
                                            (elapsed_time / 3_600_000),
                                            (elapsed_time / 60_000) % 60,
                                            (elapsed_time / 1000) % 60,
                                        );
                                        let mut request = [0u8; 1024];
                                        // this is probably not the "right way" to handle this -- but the "keep-alive" from the browser makes us hang on the read
                                        // which prevents us from answering requests from other browsers (because this is a single-threaded implementation of a server)
                                        stream.set_read_timeout(Some(Duration::from_millis(2_000))).unwrap();
                                        match stream.read(&mut request) {
                                            Ok(len) => {
                                                let r = std::string::String::from_utf8_lossy(&request[..len]);
                                                log::info!("Request received from {:?}: {}", stream.peer_addr().unwrap(), r);
                                                // this works because the recipient will take only one type of memory message and it's a 512-byte length string.
                                                xous_ipc::String::<512>::from_str(&r).send(callback_conn).unwrap();
                                                let mut tokens = r.split(' ');
                                                let mut valid = true;
                                                if let Some(verb) = tokens.next() {
                                                    if verb != "GET" {
                                                        log::info!("Expected GET, got {}", verb);
                                                        valid = false;
                                                    }
                                                }
                                                if let Some(path) = tokens.next() {
                                                    if path != "/" {
                                                        log::info!("We only know /, got {}", path);
                                                        valid = false;
                                                    }
                                                }
                                                if valid {
                                                    // now send a page back...
                                                    let page = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\n\rContent-Length: {}\n\rConnection: close\n\r\n\r{}\n\r",
                                                        test_string.len(),
                                                        test_string);
                                                    log::info!("Responding with: {}", page);
                                                    write!(stream, "{}", page).unwrap();
                                                    stream.flush().unwrap();
                                                    log::info!("Sent a response {:?}", addr);
                                                    xous_ipc::String::<512>::from_str(format!("Sent 200 to {:?}", addr)).send(callback_conn).unwrap();
                                                } else {
                                                    let errstring = "Sorry, I have only one trick, and it's pretty dumb.";
                                                    let notfound = format!("HTTP/1.1 404 Not Found\n\rConnection: close\n\rContent-Type: text/plain; charset=utf-8\n\rContent-Length: {}\n\r\n\r{}\n\r",
                                                        errstring.len(),
                                                        errstring
                                                    );
                                                    log::info!("Responding with: {}", notfound);
                                                    write!(stream, "{}", notfound).unwrap();
                                                    stream.flush().unwrap();
                                                    xous_ipc::String::<512>::from_str(format!("Sent 404 to {:?}", addr)).send(callback_conn).unwrap();
                                                }
                                            }
                                            Err(e) => {
                                                log::info!("Stream was opened, but no request received: {:?}", e);
                                                xous_ipc::String::<512>::from_str(format!("Socket opened but no request data received: {:?}", e)).send(callback_conn).unwrap();
                                            }
                                        }
                                        // stream should close automatically as `stream` goes out of scope here and Drop is called.
                                    }
                                    Err(e) => {
                                        xous_ipc::String::<512>::from_str(format!("Got error on TCP accept: {:?}", e)).send(callback_conn).unwrap();
                                    }
                                }
                            }
                        }
                    });
                    write!(ret, "TCP listener started on port 80").unwrap();
                }
                // Testing of udp is done with netcat:
                // to send packets run `netcat -u <precursor ip address> 6502` on a remote host, and then type some data
                // to receive packets, use `netcat -u -l 6502`, on the same remote host, and it should show a packet of counts received
                "udp" => {
                    if let Some(udp_socket) = &self.udp {
                        write!(ret, "Socket listener already installed on {:?}.", udp_socket.socket_addr().unwrap()).unwrap();
                    } else {
                        let socket = if let Some(tok_str) = tokens.next() {
                            tok_str
                        } else {
                            "127.0.0.1:6502"
                        };
                        let mut udp = net::UdpSocket::bind_xous(
                            socket,
                            Some(UDP_TEST_SIZE as u16)
                        ).unwrap();
                        udp.set_read_timeout(Some(Duration::from_millis(1000))).unwrap();
                        udp.set_scalar_notification(
                            self.callback_conn,
                            self.callback_id.unwrap() as usize, // this is guaranteed in the prelude
                            [Some(NetCmdDispatch::UdpTest1.to_usize().unwrap()), None, None, None]
                        );
                        self.udp = Some(udp);
                        write!(ret, "Created UDP socket listener on socket {}", socket).unwrap();
                    }
                }
                "udpclose" => {
                    self.udp = None;
                    write!(ret, "Closed primary UDP socket").unwrap();
                }
                "udpclone" => {
                    if let Some(udp_socket) = &self.udp {
                        let mut udp_clone = udp_socket.duplicate().unwrap();
                        udp_clone.set_scalar_notification(
                            self.callback_conn,
                            self.callback_id.unwrap() as usize, // this is guaranteed in the prelude
                            [Some(NetCmdDispatch::UdpTest2.to_usize().unwrap()), None, None, None]
                        );
                        let sa = udp_clone.socket_addr().unwrap();
                        self.udp_clone = Some(udp_clone);
                        write!(ret, "Cloned UDP socket on {:?}", sa).unwrap();
                    } else {
                        write!(ret, "Run `net udp` before cloning.").unwrap();
                    }
                }
                "udpcloneclose" => {
                    self.udp_clone = None;
                    write!(ret, "Closed cloned UDP socket").unwrap();
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
                        if let Some(udp_socket) = &mut self.udp {
                            let mut pkt: [u8; UDP_TEST_SIZE] = [0; UDP_TEST_SIZE];
                            match udp_socket.recv_from(&mut pkt) {
                                Ok((len, addr)) => {
                                    write!(ret, "UDP rx {} bytes: {:?}: {}", len, addr, std::str::from_utf8(&pkt[..len]).unwrap()).unwrap();
                                    log::info!("UDP rx {} bytes: {:?}: {:?}", len, addr, &pkt[..len]);
                                    self.udp_count += 1;

                                    if addr.ip() != IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)) {
                                        let response_addr = SocketAddr::new(
                                            addr.ip(),
                                            udp_socket.socket_addr().unwrap().port()
                                        );
                                        match udp_socket.send_to(
                                            format!("Received {} packets\n\r", self.udp_count).as_bytes(),
                                            &response_addr
                                        ) {
                                            Ok(len) => write!(ret, "UDP tx {} bytes", len).unwrap(),
                                            Err(_) => write!(ret, "UDP tx err").unwrap(),
                                        }
                                    } else {
                                        log::info!("localhost UDP origin detected (are you testing in hosted mode?), not reflecting packet as this would create a loop");
                                    }
                                },
                                Err(e) => {
                                    log::error!("Net UDP error: {:?}", e);
                                    write!(ret, "UDP receive error: {:?}", e).unwrap();
                                }
                            }
                        } else {
                            log::error!("Got NetCmd callback from uninitialized socket");
                            write!(ret, "Got NetCmd callback from uninitialized socket").unwrap();
                        }
                    },
                    Some(NetCmdDispatch::UdpTest2) => {
                        if let Some(udp_socket) = &mut self.udp_clone {
                            let mut pkt: [u8; UDP_TEST_SIZE] = [0; UDP_TEST_SIZE];
                            match udp_socket.recv_from(&mut pkt) {
                                Ok((len, addr)) => {
                                    write!(ret, "Clone UDP rx {} bytes: {:?}: {}", len, addr, std::str::from_utf8(&pkt[..len]).unwrap()).unwrap();
                                    log::info!("Clone UDP rx {} bytes: {:?}: {:?}", len, addr, &pkt[..len]);
                                    self.udp_count += 1;

                                    let response_addr = SocketAddr::new(
                                        addr.ip(),
                                        udp_socket.socket_addr().unwrap().port()
                                    );
                                    match udp_socket.send_to(
                                        format!("Clone received {} packets\n\r", self.udp_count).as_bytes(),
                                        &response_addr
                                    ) {
                                        Ok(len) => write!(ret, "UDP tx {} bytes", len).unwrap(),
                                        Err(e) => write!(ret, "UDP tx err: {:?}", e).unwrap(),
                                    }
                                },
                                Err(e) => {
                                    log::error!("Net UDP error: {:?}", e);
                                    write!(ret, "UDP receive error: {:?}", e).unwrap();
                                }
                            }
                        } else {
                            log::error!("Got NetCmd callback from uninitialized socket");
                            write!(ret, "Got NetCmd callback from uninitialized socket").unwrap();
                        }
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
