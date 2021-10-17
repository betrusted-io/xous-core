use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
use net::Duration;
use xous::MessageEnvelope;
use num_traits::*;
use std::net::ToSocketAddrs;

#[derive(Debug)]
pub struct NetCmd {
    udp: Option<net::UdpSocket>,
    callback_id: Option<u32>,
    callback_conn: u32,
}
impl NetCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        NetCmd {
            udp: None,
            callback_id: None,
            callback_conn: xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap(),
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCmdDispatch {
    UdpTest1,
}

pub const UDP_TEST_SIZE: usize = 64;
impl<'a> ShellCmdApi<'a> for NetCmd {
    cmd_api!(net); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        if self.callback_id.is_none() {
            let cb_id = env.register_handler(String::<256>::from_str(self.verb()));
            log::info!("hooking net callback with ID {}", cb_id);
            self.callback_id = Some(cb_id);
        }

        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "net [udp]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "udp" => {
                    if self.udp.is_none() {
                        let mut udp = net::UdpSocket::bind(
                            "127.0.0.1:6969".to_socket_addrs().unwrap().into_iter().next().unwrap(),
                            Some(UDP_TEST_SIZE as u16)
                        ).unwrap();
                        udp.set_read_timeout(Some(Duration::from_millis(1000))).unwrap();
                        udp.set_scalar_notification(
                            self.callback_conn,
                            self.callback_id.unwrap() as usize, // this is guaranteed in the prelude
                            [Some(NetCmdDispatch::UdpTest1.to_usize().unwrap()), None, None, None]
                        );
                        self.udp = Some(udp);
                        write!(ret, "Created UDP socket listener on port 6969").unwrap();
                    } else {
                        write!(ret, "Socket listener already installed on 6969").unwrap();
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

        log::info!("net callback");
        let mut ret = String::<1024>::new();
        xous::msg_scalar_unpack!(msg, dispatch, _, _, _, {
            match FromPrimitive::from_usize(dispatch) {
                Some(NetCmdDispatch::UdpTest1) => {
                    if let Some(udp_socket) = &self.udp {
                        let mut pkt: [u8; UDP_TEST_SIZE] = [0; UDP_TEST_SIZE];
                        match udp_socket.recv_from(&mut pkt) {
                            Ok((len, addr)) => {
                                write!(ret, "UDP received {} bytes: {:?}: {}", len, addr, std::str::from_utf8(&pkt[..len]).unwrap()).unwrap();
                                log::info!("UDP received {} bytes: {:?}: {:?}", len, addr, &pkt[..len]);
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
                }
                None => {
                    log::error!("NetCmd callback with unrecognized dispatch ID");
                    write!(ret, "NetCmd callback with unrecognized dispatch ID").unwrap();
                }
            }
        });
        Ok(Some(ret))
    }
}
