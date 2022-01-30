use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
use net::{Duration, XousServerId, NetPingCallback};
use xous::MessageEnvelope;
use num_traits::*;
use std::net::{SocketAddr, IpAddr};

pub struct NetCmd {
    udp: Option<net::UdpSocket>,
    udp_clone: Option<net::UdpSocket>,
    callback_id: Option<u32>,
    callback_conn: u32,
    udp_count: u32,
    dns: dns::Dns,
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
            ping: None,
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCmdDispatch {
    UdpTest1 = 0x1_0000, // we're muxing our own dispatch + ping dispatch, so we need a custom discriminant
    UdpTest2 = 0x1_0001,
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
        let helpstring = "net [udp [port]] [udpclose] [udpclone] [udpcloneclose] [ping [host] [count]] [tcpget host/path]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "tcpget" => {
                    use std::io::Write;
                    use std::io::Read;
                    // note: to keep shellchat lightweight, we do a very minimal parsing of the URL. We assume it always has
                    // a form such as:
                    // bunniefoo.com./bunnie/test.txt
                    // It will break on everything else. The `url` crate is nice but "large" for a demo.
                    // There is no https support, obvs.
                    if let Some(url) = tokens.next() {
                        match url.split_once('/') {
                            Some((host, path)) => {
                                match self.dns.lookup(host) {
                                    Ok(ipaddr) => {
                                        log::info!("resolved {} to {:?}", host, ipaddr);
                                        match net::TcpStream::connect_xous((IpAddr::from(ipaddr), 80),
                                        Some(Duration::from_millis(5000)),
                                        None) {
                                            Ok(mut stream) => {
                                                log::info!("stream open");
                                                stream.set_read_timeout(Some(Duration::from_millis(1000))).unwrap();
                                                log::info!("sending GET request");
                                                write!(stream, "GET /{} HTTP/1.1\r\n", path).expect("stream error");
                                                write!(stream, "Host: {}\r\nAccept: */*\r\nUser-Agent: Precursor/0.9.6\r\n", host).expect("stream error");
                                                write!(stream, "Connection: close\r\n").expect("stream error");
                                                write!(stream, "\r\n").expect("stream error");
                                                log::info!("fetching response....");
                                                let mut buf = [0u8; 512];
                                                match stream.read(&mut buf) {
                                                    Ok(len) => {
                                                        log::info!("raw response ({}): {:?}", len, &buf[..len]);
                                                        write!(ret, "{}", std::string::String::from_utf8_lossy(&buf[..len])).unwrap();
                                                    }
                                                    _ => write!(ret, "Didn't get response from host").unwrap(),
                                                }
                                            }
                                            _ => write!(ret, "Couldn't connect to {}:80", host).unwrap(),
                                        }
                                    }
                                    _ => write!(ret, "Couldn't resolve {}", host).unwrap(),
                                }
                            }
                            _ => write!(ret, "Usage: tcpget bunniefoo.com/bunnie/test.txt").unwrap(),
                        }
                    } else {
                        write!(ret, "Usage: tcpget bunniefoo.com/bunnie/test.txt").unwrap();
                    }
                }
                // Testing of udp is done with netcat:
                // to send packets run `netcat -u <precursor ip address> 6502` on a remote host, and then type some data
                // to receive packets, use `netcat -u -l 6502`, on the same remote host, and it should show a packet of counts received
                "udp" => {
                    if let Some(udp_socket) = &self.udp {
                        write!(ret, "Socket listener already installed on {:?}.", udp_socket.socket_addr().unwrap()).unwrap();
                    } else {
                        let port = if let Some(tok_str) = tokens.next() {
                            if let Ok(n) = tok_str.parse::<u16>() { n } else { 6502 }
                        } else {
                            6502
                        };
                        let mut udp = net::UdpSocket::bind_xous(
                            format!("127.0.0.1:{}", port),
                            Some(UDP_TEST_SIZE as u16)
                        ).unwrap();
                        udp.set_read_timeout(Some(Duration::from_millis(1000))).unwrap();
                        udp.set_scalar_notification(
                            self.callback_conn,
                            self.callback_id.unwrap() as usize, // this is guaranteed in the prelude
                            [Some(NetCmdDispatch::UdpTest1.to_usize().unwrap()), None, None, None]
                        );
                        self.udp = Some(udp);
                        write!(ret, "Created UDP socket listener on port {}", port).unwrap();
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
        xous::msg_scalar_unpack!(msg, arg1, arg2, arg3, arg4, {
            let dispatch = arg1;
            match FromPrimitive::from_usize(dispatch) {
                Some(NetCmdDispatch::UdpTest1) => {
                    if let Some(udp_socket) = &mut self.udp {
                        let mut pkt: [u8; UDP_TEST_SIZE] = [0; UDP_TEST_SIZE];
                        match udp_socket.recv_from(&mut pkt) {
                            Ok((len, addr)) => {
                                write!(ret, "UDP rx {} bytes: {:?}: {}\n", len, addr, std::str::from_utf8(&pkt[..len]).unwrap()).unwrap();
                                log::info!("UDP rx {} bytes: {:?}: {:?}", len, addr, &pkt[..len]);
                                self.udp_count += 1;

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
                                write!(ret, "Clone UDP rx {} bytes: {:?}: {}\n", len, addr, std::str::from_utf8(&pkt[..len]).unwrap()).unwrap();
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
                    let addr = IpAddr::from((arg2 as u32).to_be_bytes());
                    let seq_or_addr = arg3;
                    let timestamp = arg4;
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
        });
        Ok(Some(ret))
    }
}
