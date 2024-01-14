use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::net::IpAddr;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use num_traits::*;
use xous::{msg_scalar_unpack, send_message, Message};
use xous_ipc::Buffer;

use crate::api::*;
use crate::NetConn;

///////// Ping implementation
/// smoltcp's ICMP stack is a little weird. It's designed with an icmp packet
/// formatter, but it's built into the socket handle itself. The reasoning behind
/// this was to keep memory allocations small and static.
///
/// In a single-threaded implementation, one would normally have no problem simply
/// pulling up the socket handle and jamming or unpacking data using that handle.
/// Unfortunately, in Xous, we want to keep smoltcp inside the Net crate, and not
/// have it bleed into other parts of the OS. The first take at implementing this
/// thus ended up having to duplicate the storage for ICMP into the "lib" side code,
/// and using the smoltcp convenience functions to create and format packets.
///
/// This ugly division of labor leads to an unweildy handler prone to locking
/// and object lifetime issues. There's two options on how to work our way out of this.
/// 1. One is to re-implement ICMP packet packing and unpacking inside Xous, so that smoltcp is just a raw
///    ICMP packet pusher.
/// 2. Make the Xous Net interface to ICMP specific to "ping", pushing sequence numbers and origin timestamps
///    into the Net crate, and getting back sequence numbers with the corresponding timestamps.
///
/// The downside of (1) is duplication of effort. ICMP is fairly straightforward for
/// IPv4, but unfortunately for IPv6 it requires knowledge of the source address
/// which puts a burden on coordinating DHCP updates into the implementation.
/// The downside of (2) is it makes things a little awkward later on if someone
/// wants to implement fancy ICMP protocol hacks in the Net stack.
///
/// However, it looks like there is a clean place to hook a traceroute implementation
/// into the smoltcp stack, as it has a parser for TimeExceeded packts built in,
/// and we could provide a hook for that, and perhaps a Traceroute utility that
/// exists in parallel to the Ping implementation on the "lib" side of the Net crate.
///
/// Thus for this refactor, we are going to push more function of ICMP parsing and
/// formatting into the "main" side of the Net crate and thus into the ICMP socket
/// implementation, and leave the client side a bit more naive & high-level, functioning
/// closer to a full utility. This is unlike the UDP and TCP implementations, which
/// are ignorant of what's being done with them.

pub struct Ping {
    net: NetConn,
    callback_server: Option<XousServerId>,
    dispatch_opcode: Option<usize>,
}

impl Ping {
    /// The caller must guarantee that the `callback_server` -- whatever it is -- outlives the duration
    /// of any Ping return callbacks. These callbacks can even outlive the scope of this Ping
    /// object.
    ///
    /// The server can know all the callbacks are done when it
    /// receives the `NetPingCallback::Drop` sub-opcode from the Net server.
    /// `dispatch_opcode` is a number that is inserted in the Scalar message ID field to help
    /// the server route the message to the correct bit of code. This is typically an Opcode
    /// from the master dispatch enum for your server, or if it is a single-use server, the
    /// field could be ignored.
    pub fn non_blocking_handle(callback_server: XousServerId, dispatch_opcode: usize) -> Ping {
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        Ping { net, callback_server: Some(callback_server), dispatch_opcode: Some(dispatch_opcode) }
    }

    /// This is a handle just for manipulating settings on the ping socket. You would create
    /// one of these if you wanted to adjust the ping settings before issuing a `blocking` ping,
    /// which is incapable of manipulating the Net crate state.
    pub fn settings_handle() -> Ping {
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        Ping { net, callback_server: None, dispatch_opcode: None }
    }

    /// Send a single ping immediately, with the result going to an asynchronous callback
    pub fn ping(&self, remote: IpAddr) -> bool {
        if let Some(cbs) = self.callback_server {
            if let Some(dispatch_op) = self.dispatch_opcode {
                let ping = NetPingPacket {
                    endpoint: NetIpAddr::from(remote),
                    server: cbs,
                    return_opcode: dispatch_op,
                    sent_ok: None,
                };
                let mut buf = Buffer::into_buf(ping).expect("couldn't allocate memory to send Ping");
                buf.lend_mut(self.net.conn(), Opcode::Ping.to_u32().unwrap())
                    .expect("couldn't send Ping command");
                let ret = buf.to_original::<NetPingPacket, _>().unwrap();
                if let Some(sent_ok) = ret.sent_ok {
                    sent_ok
                } else {
                    panic!(
                        "Internal error: malformed data returned from Net while processing Ping operation"
                    );
                }
            } else {
                log::warn!("Ping object was not initialized for async operation.");
                false
            }
        } else {
            log::warn!("Ping object was not initialized for async operation.");
            false
        }
    }

    /// Send multiple pings by spawning a helper thread. Results go to the asynchronous callback
    /// specified during the creation of the Ping object.
    pub fn ping_spawn_thread(&self, remote: IpAddr, count: usize, delay_ms: usize) -> Option<JoinHandle<()>> {
        if let Some(cbs) = self.callback_server {
            if let Some(dispatch_op) = self.dispatch_opcode {
                let handle = thread::spawn({
                    let cbs = cbs.clone();
                    let dispatch_op = dispatch_op.clone();
                    let net_conn = self.net.conn().clone();
                    move || {
                        let tt = ticktimer_server::Ticktimer::new().unwrap();
                        let mut cur_count = 0;
                        while cur_count < count {
                            let ping = NetPingPacket {
                                endpoint: NetIpAddr::from(remote),
                                server: cbs,
                                return_opcode: dispatch_op,
                                sent_ok: None,
                            };
                            let mut buf =
                                Buffer::into_buf(ping).expect("couldn't allocate memory to send Ping");
                            buf.lend_mut(net_conn, Opcode::Ping.to_u32().unwrap())
                                .expect("couldn't send Ping command");
                            let ret = buf.to_original::<NetPingPacket, _>().unwrap();
                            if let Some(sent_ok) = ret.sent_ok {
                                if !sent_ok {
                                    log::warn!(
                                        "Problem sending a Ping inside ping_spawn; ignoring error and moving on."
                                    );
                                }
                            } else {
                                panic!(
                                    "Internal error: malformed data returned from Net while processing Ping operation"
                                );
                            }
                            tt.sleep_ms(delay_ms).unwrap();
                            cur_count += 1;
                        }
                    }
                });
                Some(handle)
            } else {
                log::warn!("Ping object was not initialized for async operation.");
                None
            }
        } else {
            log::warn!("Ping object was not initialized for async operation.");
            None
        }
    }

    /// This sends a ping, and blocks execution until it either times out, or a ping is received.
    /// It immediately destroys all of its temporary allocated connections to the Net crate upon exit,
    /// so it does not return a Ping object.
    pub fn blocking(remote: IpAddr) -> (bool, u32) {
        let reachable = Arc::new(AtomicBool::new(false));
        let ping_time = Arc::new(AtomicU32::new(0));
        let handle = thread::spawn({
            let reachable = Arc::clone(&reachable);
            let ping_time = Arc::clone(&ping_time);
            move || {
                let xns = xous_names::XousNames::new().unwrap();
                let net = NetConn::new(&xns).unwrap();
                let sid = xous::create_server().unwrap();
                let ping = NetPingPacket {
                    endpoint: NetIpAddr::from(remote),
                    server: XousServerId::PrivateSid(sid.to_array()),
                    return_opcode: 0,
                    sent_ok: None,
                };
                let mut buf = Buffer::into_buf(ping).expect("couldn't allocate memory to send Ping");
                buf.lend_mut(net.conn(), Opcode::Ping.to_u32().unwrap()).expect("couldn't send Ping command");
                let ret = buf.to_original::<NetPingPacket, _>().unwrap();
                if let Some(sent_ok) = ret.sent_ok {
                    if sent_ok {
                        loop {
                            let msg = xous::receive_message(sid).unwrap();
                            // only one message type is expected back, so we don't match on ID -- just unpack
                            // the message already!
                            msg_scalar_unpack!(msg, op, _addr, seq_or_addr, timestamp, {
                                match FromPrimitive::from_usize(op & 0xFF) {
                                    Some(NetPingCallback::Drop) => {
                                        break;
                                    }
                                    Some(NetPingCallback::NoErr) => match remote {
                                        IpAddr::V4(_) => {
                                            reachable.store(true, Ordering::SeqCst);
                                            ping_time.store(timestamp as u32, Ordering::SeqCst);
                                            log::info!(
                                                "Pong from {:?} seq {} received: {} ms",
                                                remote,
                                                seq_or_addr,
                                                timestamp
                                            );
                                        }
                                        IpAddr::V6(_) => {
                                            reachable.store(true, Ordering::SeqCst);
                                            ping_time.store(timestamp as u32, Ordering::SeqCst);
                                            log::info!("Pong from {:?} received: {} ms", remote, timestamp);
                                        }
                                    },
                                    Some(NetPingCallback::Timeout) => {
                                        reachable.store(false, Ordering::SeqCst);
                                        ping_time.store(timestamp as u32, Ordering::SeqCst);
                                        log::info!("Ping to {:?} timed out", remote);
                                    }
                                    Some(NetPingCallback::Unreachable) => {
                                        let code =
                                            smoltcp::wire::Icmpv4DstUnreachable::from((op >> 24) as u8);
                                        reachable.store(false, Ordering::SeqCst);
                                        log::info!("Ping to {:?} unreachable: {:?}", remote, code);
                                    }
                                    None => {
                                        log::error!("Unknown opcode received in one-time server: {:?}", op);
                                    }
                                }
                            });
                        }
                    } else {
                        log::error!(
                            "Ping to {:?} was requested, but an internal error prevented it from being sent (maybe ICMP socket is busy?).",
                            remote
                        );
                    }
                } else {
                    panic!("Internal error -- Ping was requested, but the returned memory was malformed");
                }
                xous::destroy_server(sid).expect("couldn't destroy one-time use server");
            }
        });
        handle.join().expect("couldn't join single-use server handle");
        (reachable.load(Ordering::SeqCst), ping_time.load(Ordering::SeqCst))
    }

    pub fn set_timeout(&mut self, timeout_ms: u32) {
        send_message(
            self.net.conn(),
            Message::new_scalar(Opcode::PingSetTimeout.to_usize().unwrap(), timeout_ms as usize, 0, 0, 0),
        )
        .expect("couldn't send set timeout for ping");
    }

    pub fn get_timeout(&self) -> u32 {
        let response = send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::PingGetTimeout.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't get ping timeout");
        if let xous::Result::Scalar1(result) = response {
            result as u32
        } else {
            panic!("Could execute get_timeout call");
        }
    }

    pub fn get_ttl(&self) -> u8 {
        let response = send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::PingGetTtl.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("Couldn't get ping TTL");
        if let xous::Result::Scalar1(result) = response {
            result as u8
        } else {
            panic!("Could execute get_ttl call");
        }
    }

    pub fn set_ttl(&self, ttl: u8) {
        send_message(
            self.net.conn(),
            Message::new_scalar(Opcode::PingSetTtl.to_usize().unwrap(), ttl as usize, 0, 0, 0),
        )
        .expect("couldn't send set TTL message for ping");
    }
}
