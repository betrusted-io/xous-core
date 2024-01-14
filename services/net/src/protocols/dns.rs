use std::collections::HashSet;
use std::io::{Error, ErrorKind, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

//use crate::api::udp::*;
use num_traits::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, Message, SID};
use xous_ipc::Buffer;

use crate::api::*;
use crate::NetConn;

pub struct DnsServerManager {
    net: NetConn,
    servers: Arc<Mutex<HashSet<IpAddr>>>,
    cb_sid: SID,
    handle: Option<JoinHandle<()>>,
    freeze: bool,
}

impl DnsServerManager {
    pub fn register(xns: &xous_names::XousNames) -> Result<DnsServerManager> {
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();

        let servers = Arc::new(Mutex::new(HashSet::<IpAddr>::new()));

        let handle = thread::spawn({
            let cb_sid_clone = cb_sid.clone();
            let servers = Arc::clone(&servers);
            move || {
                loop {
                    let msg = xous::receive_message(cb_sid_clone).unwrap();
                    match FromPrimitive::from_usize(msg.body.id()) {
                        Some(PrivateDnsOp::AddIpv4DnsServer) => {
                            msg_scalar_unpack!(msg, be_octets, _, _, _, {
                                let octets = (be_octets as u32).to_be_bytes();
                                let dns_ip =
                                    IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]));
                                servers.lock().unwrap().insert(dns_ip);
                            })
                        }
                        Some(PrivateDnsOp::AddIpv6DnsServer) => msg_scalar_unpack!(msg, w0, w1, w2, w3, {
                            let w0_be = (w0 as u32).to_be_bytes();
                            let w1_be = (w1 as u32).to_be_bytes();
                            let w2_be = (w2 as u32).to_be_bytes();
                            let w3_be = (w3 as u32).to_be_bytes();
                            let addr_bytes = [
                                w0_be[0], w0_be[1], w0_be[2], w0_be[3], w1_be[0], w1_be[1], w1_be[2],
                                w1_be[3], w2_be[0], w2_be[1], w2_be[2], w2_be[3], w3_be[0], w3_be[1],
                                w3_be[2], w3_be[3],
                            ];
                            let dns_ip = IpAddr::V6(Ipv6Addr::from(addr_bytes));
                            servers.lock().unwrap().insert(dns_ip);
                        }),
                        Some(PrivateDnsOp::RemoveIpv4DnsServer) => {
                            msg_scalar_unpack!(msg, be_octets, _, _, _, {
                                let octets = (be_octets as u32).to_be_bytes();
                                let dns_ip =
                                    IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]));
                                if !servers.lock().unwrap().remove(&dns_ip) {
                                    log::warn!(
                                        "Attempt to remove DNS server {:?} that isn't in our list. Ignored.",
                                        dns_ip
                                    );
                                }
                            })
                        }
                        Some(PrivateDnsOp::RemoveIpv6DnsServer) => msg_scalar_unpack!(msg, w0, w1, w2, w3, {
                            let w0_be = (w0 as u32).to_be_bytes();
                            let w1_be = (w1 as u32).to_be_bytes();
                            let w2_be = (w2 as u32).to_be_bytes();
                            let w3_be = (w3 as u32).to_be_bytes();
                            let addr_bytes = [
                                w0_be[0], w0_be[1], w0_be[2], w0_be[3], w1_be[0], w1_be[1], w1_be[2],
                                w1_be[3], w2_be[0], w2_be[1], w2_be[2], w2_be[3], w3_be[0], w3_be[1],
                                w3_be[2], w3_be[3],
                            ];
                            let dns_ip = IpAddr::V6(Ipv6Addr::from(addr_bytes));
                            if !servers.lock().unwrap().remove(&dns_ip) {
                                log::warn!(
                                    "Attempt to remove DNS server {:?} that isn't in our list. Ignored.",
                                    dns_ip
                                );
                            }
                        }),
                        Some(PrivateDnsOp::RemoveAllServers) => msg_scalar_unpack!(msg, _, _, _, _, {
                            servers.lock().unwrap().clear();
                        }),
                        Some(PrivateDnsOp::Quit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                            log::debug!("Quit received, exiting DNS notifier");
                            xous::return_scalar(msg.sender, 1).unwrap(); // actual return value doesn't matter -- it's that there is a return value
                            break;
                        }),
                        None => {
                            log::error!("got unknown message type on DNS callback: {:?}", msg);
                        }
                    }
                }
            }
        });
        let mut success = true;
        let hook = XousPrivateServerHook {
            one_time_sid: cb_sid.to_array(),
            op: PrivateDnsOp::AddIpv4DnsServer.to_usize().unwrap(),
            args: [None; 4],
        };
        let mut buf =
            Buffer::into_buf(hook).or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::DnsHookAddIpv4.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {}
            _ => success = false,
        }
        let hook = XousPrivateServerHook {
            one_time_sid: cb_sid.to_array(),
            op: PrivateDnsOp::AddIpv6DnsServer.to_usize().unwrap(),
            args: [None; 4],
        };
        let mut buf =
            Buffer::into_buf(hook).or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::DnsHookAddIpv6.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {}
            _ => success = false,
        }
        let hook = XousPrivateServerHook {
            one_time_sid: cb_sid.to_array(),
            op: PrivateDnsOp::RemoveAllServers.to_usize().unwrap(),
            args: [None; 4],
        };
        let mut buf =
            Buffer::into_buf(hook).or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        buf.lend_mut(net.conn(), Opcode::DnsHookAllClear.to_u32().unwrap())
            .or(Err(Error::new(ErrorKind::Other, "can't hook with Net server")))?;
        match buf.to_original().unwrap() {
            NetMemResponse::Ok => {}
            _ => success = false,
        }
        if success {
            Ok(DnsServerManager { net, cb_sid, servers, handle: Some(handle), freeze: false })
        } else {
            Err(Error::new(ErrorKind::AlreadyExists, "Failed to hook DNS; did someone else do it alread?"))
        }
    }

    /// Returns true if server was not already present, false if it's already there.
    pub fn add_server(&mut self, addr: IpAddr) -> bool {
        if !self.freeze { self.servers.lock().unwrap().insert(addr) } else { false }
    }

    /// Returns true if the server was removed, false if the server wasn't in the table and thus couldn't be
    /// removed.
    pub fn remove_server(&mut self, addr: IpAddr) -> bool {
        if !self.freeze { self.servers.lock().unwrap().remove(&addr) } else { false }
    }

    pub fn clear(&mut self) {
        if !self.freeze {
            self.servers.lock().unwrap().clear();
        }
    }

    pub fn set_freeze(&mut self, freeze: bool) { self.freeze = freeze; }

    /// Get one of the DNS servers. Which one we get, we don't know!
    pub fn get_random(&self) -> Option<IpAddr> {
        if let Some(&addr) = self.servers.lock().unwrap().iter().next() { Some(addr) } else { None }
    }
}

impl Drop for DnsServerManager {
    fn drop(&mut self) {
        xous::send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::DnsHookAllClear.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send unhook to Net crate");

        let drop_cid = xous::connect(self.cb_sid).unwrap();
        xous::send_message(
            drop_cid,
            Message::new_blocking_scalar(PrivateDnsOp::Quit.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("couldn't send Quit to our DNS manager thread");
        unsafe { xous::disconnect(drop_cid).unwrap() }; // should be safe because we're the only connection and the previous was a blocking scalar

        // this will block until the responder thread exits, which it should because it received the Drop
        // message
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
        // now we can detroy the server id of the responder thread
        xous::destroy_server(self.cb_sid).unwrap();
    }
}
