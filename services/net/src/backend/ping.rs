use std::convert::TryInto;
use std::net::IpAddr;
use std::unimplemented;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::collections::HashMap;
use core::sync::atomic::{AtomicBool, Ordering};

use byteorder::{ByteOrder, NetworkEndian};
use smoltcp::phy::ChecksumCapabilities;
use smoltcp::wire::{
    Icmpv4Packet, Icmpv4Repr, IpAddress,
    Icmpv6Packet, Icmpv6Repr, Ipv6Address
};

use xous::{CID, Message, SID, msg_blocking_scalar_unpack, send_message};
use xous_ipc::Buffer;
use crate::NetConn;
use crate::api::*;
use num_traits::*;

///////// Ping implementation
/// Ping will accept an IP Address (IPv4 or IPv6) and attempt to ping
/// the remote server. It will manage its own sequence number. The
/// starting number is picked at random for each instance, which means
/// if you have multiple pings running at once there is a chance of
/// sequence collisions, but this is considered an edge case.
///
/// Ping responses are meant to trigger a callback message which contains
/// the ping time and remote host; however, if None is specified for the
/// ping responder, the process simply blocks until a response happens or
/// a timeout is met.
///
/// The default timeout is 10 seconds.
const PING_DEFAULT_TIMEOUT_MS: u64 = 10_000;
const PING_POLL_INTERVAL_MS: usize = 1_000;
pub struct Ping {
    net: NetConn,
    cb_sid: SID,
    self_cid: CID,
    handle: Option<JoinHandle::<()>>,
    timer: ticktimer_server::Ticktimer,
    timeout: Arc<Mutex<u64>>,
    notify: Arc<Mutex<XousScalarEndpoint>>,
    waiting_queue: Arc<Mutex<HashMap<u16, u64>>>,
    seq_no: u16,
    buf: [u8; PING_MAX_PKT_LEN],
    checksum_caps: Arc<Mutex<ChecksumCapabilities>>,
    src_addr: Arc<Mutex<Option<IpAddr>>>,
    remote_addr: Arc<Mutex<Option<IpAddress>>>,
    one_shot_active: Arc<AtomicBool>,
}

impl Ping {
    /// Ping is fully asynchronous. Responses need to be hooked with a callback.
    pub fn new() -> Ping {
        let xns = xous_names::XousNames::new().unwrap();
        let net = NetConn::new(&xns).unwrap();
        let cb_sid = xous::create_server().unwrap();
        let trng = trng::Trng::new(&xns).unwrap();

        let waiting_queue = Arc::new(Mutex::new(HashMap::new()));
        let notify = Arc::new(Mutex::new(XousScalarEndpoint::new()));
        let src_addr = Arc::new(Mutex::new(None));
        let remote_addr = Arc::new(Mutex::new(None));
        let checksum_caps = Arc::new(Mutex::new(ChecksumCapabilities::default())); // defaults to "Both"
        let timeout = Arc::new(Mutex::new(PING_DEFAULT_TIMEOUT_MS));
        let one_shot_active = Arc::new(AtomicBool::new(false));
        let self_cid = xous::connect(cb_sid).unwrap();
        let handle = thread::spawn({
            let cb_sid_clone = cb_sid.clone();
            let waiting_queue = Arc::clone(&waiting_queue);
            let notify = Arc::clone(&notify);
            let src_addr = Arc::clone(&src_addr);
            let checksum_caps = Arc::clone(&checksum_caps);
            let remote_addr = Arc::clone(&remote_addr);
            let timeout = Arc::clone(&timeout);
            let one_shot_active = Arc::clone(&one_shot_active);
            let self_cid = self_cid.clone();
            move || {
                loop {
                    let tt = ticktimer_server::Ticktimer::new().unwrap();
                    let msg = xous::receive_message(cb_sid_clone).unwrap();
                    match FromPrimitive::from_usize(msg.body.id()) {
                        Some(NetPingCallback::RxData) => {
                            let buffer = unsafe {Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                            let incoming = buffer.to_original::<NetPingResponsePacket, _>().unwrap();
                            let payload = &incoming.data[..incoming.len as usize];
                            log::trace!("pinger got {:?}", payload);
                            let remote_addr_clone = match *remote_addr.lock().unwrap() {
                                Some(ra) => ra,
                                None => {
                                    log::warn!("remote address was not set, yet somehow a remote has responded!");
                                    continue
                                },
                            };
                            match remote_addr_clone {
                                IpAddress::Ipv4(_) => {
                                    log::trace!("decode ipv4 ping");
                                    let icmp_packet = Icmpv4Packet::new_checked(&payload).unwrap();
                                    log::trace!("icmp_packet: {:?}", icmp_packet);
                                    let icmp_repr =
                                        Icmpv4Repr::parse(&icmp_packet, &checksum_caps.lock().unwrap()).unwrap();
                                    log::trace!("icmp_repr: {:?}", icmp_repr);
                                    if let Icmpv4Repr::EchoReply{
                                        seq_no,
                                        data,
                                        ..
                                    } = icmp_repr {
                                        log::trace!("ping back of seq {}", seq_no);
                                        let seq_valid = waiting_queue.lock().unwrap().contains_key(&seq_no);
                                        // bind to seq_valid so lock is droppped, alowing lock to be got again inside the if below
                                        if seq_valid {
                                            let packet_timestamp_ms = NetworkEndian::read_u64(data);
                                            waiting_queue.lock().unwrap().remove(&seq_no);
                                            log::trace!("sending notification of seq {}", seq_no);
                                            notify.lock().unwrap().notify_custom_args([
                                                None, // replaced with discriminant setup in the callback
                                                Some((tt.elapsed_ms() - packet_timestamp_ms) as u32),
                                                Some(u32::from_be_bytes(remote_addr_clone.as_bytes().try_into().unwrap())),
                                                None,
                                            ]);
                                        } else {
                                            log::warn!("pong seq {} not found!", seq_no);
                                        }
                                    } else {
                                        log::warn!("packet response did not match icmp template");
                                    }
                                }
                                IpAddress::Ipv6(_) => {
                                    if let Some(src_addr) = *src_addr.lock().unwrap() {
                                        let src_ipv6 = match src_addr {
                                            // not sure if this a valid way to convert v4->v6, but "meh"
                                            IpAddr::V4(ipv4) => {
                                                let octets = ipv4.octets();
                                                IpAddress::Ipv6(Ipv6Address::new(
                                                    0, 0, 0, 0, 0,
                                                    0xffff,
                                                    u16::from_be_bytes([octets[0], octets[1]]),
                                                    u16::from_be_bytes([octets[2], octets[3]])
                                                ))
                                            }
                                            IpAddr::V6(ipv6) => IpAddress::from(ipv6)
                                        };
                                        let icmp_packet = Icmpv6Packet::new_checked(&payload).unwrap();
                                        let icmp_repr = Icmpv6Repr::parse(
                                        &remote_addr.lock().unwrap().unwrap(),
                                        &src_ipv6,
                                        &icmp_packet,
                                        &checksum_caps.lock().unwrap(),
                                        ).unwrap();

                                        if let Icmpv6Repr::EchoReply{
                                            seq_no,
                                            data,
                                            ..
                                        } = icmp_repr {
                                            let seq_valid = waiting_queue.lock().unwrap().contains_key(&seq_no);
                                            // bind to seq_valid so lock is droppped, alowing lock to be got again inside the if below
                                            if seq_valid {
                                                let packet_timestamp_ms = NetworkEndian::read_u64(data);
                                                waiting_queue.lock().unwrap().remove(&seq_no);
                                                let octet_slice = remote_addr_clone.as_bytes();
                                                let mut octets: [u8; 16] = [0; 16];
                                                for (&src, dst) in octet_slice.iter().zip(octets.iter_mut()) {
                                                    *dst = src;
                                                }
                                                notify.lock().unwrap().notify_custom_args([
                                                    None, // replaced with discriminant
                                                    Some((tt.elapsed_ms() - packet_timestamp_ms) as u32), // drop the first 4 octets and replace with response time...
                                                    Some(u32::from_be_bytes(octets[8..12].try_into().unwrap())),
                                                    Some(u32::from_be_bytes(octets[12..16].try_into().unwrap())),
                                                ]);
                                            }
                                        }
                                    } else {
                                        log::error!("Got IPV6 response, but our source address wasn't set.")
                                    }
                                }
                                _ => {
                                    log::warn!("Internal error on formatting of remote address. Memory corruption?");
                                }
                            }
                            log::info!("managing wait queue");
                            let timestamp = tt.elapsed_ms();
                            waiting_queue.lock().unwrap().retain(|seq, from| {
                                if timestamp - *from < *timeout.lock().unwrap() {
                                    log::trace!("sequence {} still pending", seq);
                                    true
                                } else {
                                    log::trace!("expiring sequence {}", seq);
                                    // TODO: fix IPV6 case -- need to pack out partial args of the address or something...
                                    notify.lock().unwrap().notify_custom_args([
                                        None, // replaced with discriminant setup in the callback
                                        None, // indicates failure -> resolves to 0 time
                                        Some(u32::from_be_bytes(remote_addr.lock().unwrap().unwrap().as_bytes().try_into().unwrap())),
                                        None,
                                    ]);
                                    false
                                }
                            });
                        },
                        Some(NetPingCallback::CheckTimeout) => {
                            let timestamp = tt.elapsed_ms();
                            waiting_queue.lock().unwrap().retain(|_seq, from| {
                                if timestamp - *from < *timeout.lock().unwrap() {
                                    true
                                } else {
                                    // TODO: fix IPV6 case -- need to pack out partial args of the address or something...
                                    notify.lock().unwrap().notify_custom_args([
                                        None, // replaced with discriminant setup in the callback
                                        None, // indicates failure -> resolves to 0 time
                                        Some(u32::from_be_bytes(remote_addr.lock().unwrap().unwrap().as_bytes().try_into().unwrap())),
                                        None,
                                    ]);
                                    false
                                }
                            });
                            if waiting_queue.lock().unwrap().len() > 0 {
                                if !one_shot_active.swap(true, Ordering::SeqCst) {
                                    log::trace!("spawing one-shot");
                                    thread::spawn({
                                        let self_cid = self_cid.clone();
                                        let one_shot_active = one_shot_active.clone();
                                        move || {
                                            let tt = ticktimer_server::Ticktimer::new().unwrap();
                                            tt.sleep_ms(PING_POLL_INTERVAL_MS).unwrap();
                                            one_shot_active.store(false, Ordering::SeqCst);
                                            match xous::send_message(self_cid,
                                                Message::new_scalar(NetPingCallback::CheckTimeout.to_usize().unwrap(), 0, 0, 0, 0)
                                            ) {
                                                Ok(_) => {},
                                                Err(xous::Error::ServerNotFound) => {
                                                    log::warn!("Responder went out of scope for timeout poll. Maybe the lifetime of your Ping object was too short?");
                                                }
                                                _ => panic!("unhandled error in sending ping poll")
                                            }
                                        }
                                    });
                                    log::trace!("one-shot spawned");
                                }
                            }
                        }
                        Some(NetPingCallback::SrcAddr) => {
                            // source address is required for ipv6 icmp packet generation
                            let buffer = unsafe {Buffer::from_memory_message(msg.body.memory_message().unwrap())};
                            let incoming = buffer.to_original::<NetIpAddr, _>().unwrap();
                            *src_addr.lock().unwrap() = Some(IpAddr::from(incoming));
                        }
                        Some(NetPingCallback::Drop) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                            log::debug!("Drop received, exiting ping handler");
                            xous::return_scalar(msg.sender, 1).unwrap(); // actual return value doesn't matter -- it's that there is a return value
                            break;
                        }),
                        None => {
                            log::error!("got unknown message type on Ping callback: {:?}", msg);
                        }
                    }
                }
            }
        });

        let sid = cb_sid.to_array();
        send_message(
            net.conn(),
            Message::new_scalar(Opcode::PingRegisterRx.to_usize().unwrap(),
            sid[0] as _, sid[1] as _, sid[2] as _,sid[3] as _,
            )
        ).expect("couldn't register Ping listener");

        Ping {
            net,
            cb_sid,
            self_cid,
            handle: Some(handle),
            timer: ticktimer_server::Ticktimer::new().unwrap(),
            timeout,
            notify,
            waiting_queue,
            seq_no: trng.get_u32().unwrap() as u16,
            buf: [0; PING_MAX_PKT_LEN],
            checksum_caps,
            src_addr,
            remote_addr,
            one_shot_active,
        }
    }
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        *self.timeout.lock().unwrap() = timeout_ms;
    }
    pub fn get_timeout(&self) -> u64 {
        *self.timeout.lock().unwrap()
    }
    pub fn set_scalar_notification(&mut self, cid: CID, op: usize, discriminant: u32) {
        self.notify.lock().unwrap().set(cid, op, [Some(discriminant as usize), None, None, None]);
    }
    pub fn clear_scalar_notification(&mut self) {
        self.notify.lock().unwrap().clear();
    }
    pub fn get_ttl(&self) -> u8 {
        let response = send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::PingGetTtl.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("Couldn't get ping TTL");
        if let xous::Result::Scalar1(result) = response {
            result as u8
        } else {
            panic!("Could execute get_ttl call");
        }
    }
    pub fn set_ttl(&self, ttl: u8) {
        send_message(
            self.net.conn(),
            Message::new_scalar(Opcode::PingSetTtl.to_usize().unwrap(), ttl as usize, 0, 0, 0)
        ).expect("couldn't send set TTL message for ping");
    }
    /// blocks until all the pings have either pong'd or timed out -- not sure if this is actually a good idea to have instead of
    /// relying on the async callback in all cases?
    /*
    pub fn wait_pong(&self) {
        loop {
            let no_pongs_pending = self.waiting_queue.lock().unwrap().is_empty();
            if no_pongs_pending {
                break;
            } else {
                self.timer.sleep_ms(PING_POLL_INTERVAL_MS / 4).unwrap();
            }
        }
    }*/
    pub fn ping(&mut self, remote: IpAddr) {
        let ident = 0x22b;
        let timestamp = self.timer.elapsed_ms();
        let mut echo_payload = [0xffu8; 40];
        NetworkEndian::write_u64(&mut echo_payload, timestamp);
        let remote_addr = IpAddress::from(remote);
        match remote_addr {
            IpAddress::Ipv4(_) => {
                log::debug!("ping sending to {:?} seq_no {}", remote_addr, self.seq_no);
                let icmp_repr = Icmpv4Repr::EchoRequest {
                    ident,
                    seq_no: self.seq_no,
                    data: &echo_payload
                };
                let mut icmp_packet = Icmpv4Packet::new_unchecked(&mut self.buf);
                icmp_repr.emit(&mut icmp_packet, &self.checksum_caps.lock().unwrap());
                let icmp_inner = icmp_packet.into_inner();

                let mut pkt = NetPingPacket {
                    len: icmp_inner.len() as u32,
                    data: [0; PING_MAX_PKT_LEN],
                    endpoint: NetIpAddr::from(remote),
                };
                for (&src, dst) in icmp_inner.iter().zip(pkt.data.iter_mut()) {
                    *dst = src;
                }
                let buf = Buffer::into_buf(pkt).expect("couldn't allocate Buffer");
                buf.send(self.net.conn(), Opcode::PingTx.to_u32().unwrap()).expect("couldn't send Ping packet");
            }
            IpAddress::Ipv6(_) => {
                // this code probably does not work, as it has not been tested
                if let Some(src_addr) = *self.src_addr.lock().unwrap() {
                    let src_ipv6 = match src_addr {
                        // not sure if this a valid way to convert v4->v6, but "meh"
                        IpAddr::V4(ipv4) => {
                            let octets = ipv4.octets();
                            IpAddress::Ipv6(Ipv6Address::new(
                                0, 0, 0, 0, 0,
                                0xffff,
                                u16::from_be_bytes([octets[0], octets[1]]),
                                u16::from_be_bytes([octets[2], octets[3]])
                            ))
                        }
                        IpAddr::V6(ipv6) => IpAddress::from(ipv6)
                    };
                    let icmp_repr = Icmpv6Repr::EchoRequest {
                        ident,
                        seq_no: self.seq_no,
                        data: &echo_payload
                    };
                    let mut icmp_packet = Icmpv6Packet::new_unchecked(&mut self.buf);
                    icmp_repr.emit(
                        &src_ipv6,
                        &remote_addr,
                        &mut icmp_packet,
                        &self.checksum_caps.lock().unwrap()
                    );
                    let icmp_inner = icmp_packet.into_inner();
                    let mut pkt = NetPingPacket {
                        len: icmp_inner.len() as u32,
                        data: [0; PING_MAX_PKT_LEN],
                        endpoint: NetIpAddr::from(remote),
                    };
                    for (&src, dst) in icmp_inner.iter().zip(pkt.data.iter_mut()) {
                        *dst = src;
                    }
                    let buf = Buffer::into_buf(pkt).expect("couldn't allocate Buffer");
                    buf.send(self.net.conn(), Opcode::PingTx.to_u32().unwrap()).expect("couldn't send Ping packet");
                } else {
                    log::error!("Attempt to send ipv6 ping but no source address is available");
                }
            }
            _ => unimplemented!(),
        }
        self.waiting_queue.lock().unwrap().insert(self.seq_no, timestamp);
        self.seq_no = self.seq_no.wrapping_add(1);

        // record the remote_addr so we know if we're parsing an ipv4 or ipv6 packet
        *self.remote_addr.lock().unwrap() = Some(remote_addr);
        log::debug!("spawning first one-shot");
        self.one_shot_active.store(true, Ordering::SeqCst);
        thread::spawn({
            let self_cid = self.self_cid.clone();
            let one_shot_active = self.one_shot_active.clone();
            move || {
                let tt = ticktimer_server::Ticktimer::new().unwrap();
                tt.sleep_ms(PING_POLL_INTERVAL_MS).unwrap();
                one_shot_active.store(false, Ordering::SeqCst);
                match xous::try_send_message(self_cid,
                    Message::new_scalar(NetPingCallback::CheckTimeout.to_usize().unwrap(), 0, 0, 0, 0)
                ) {
                    Ok(_) => {},
                    Err(xous::Error::ServerNotFound) => {
                        log::warn!("Responder went out of scope for timeout poll. Maybe the lifetime of your Ping object was too short?");
                    }
                    _ => panic!("unhandled error in sending ping poll")
                }
            }
        });
        log::debug!("spawning first one-shot spawned");
    }
}

impl Drop for Ping {
    fn drop(&mut self) {

        let sid = self.cb_sid.to_array();
        send_message(
            self.net.conn(),
            Message::new_blocking_scalar(Opcode::PingUnregisterRx.to_usize().unwrap(),
            sid[0] as _, sid[1] as _, sid[2] as _,sid[3] as _,
            )
        ).expect("couldn't unregister Ping listener");

        let drop_cid = xous::connect(self.cb_sid).unwrap();
        xous::send_message(
            drop_cid,
            Message::new_blocking_scalar(NetPingCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't send Drop to our repsonding server");
        unsafe{xous::disconnect(drop_cid).unwrap()}; // should be safe because we're the only connection and the previous was a blocking scalar

        // this will block until the responder thread exits, which it should because it received the Drop message
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap();
        }
        // now we can detroy the server id of the responder thread
        xous::destroy_server(self.cb_sid).unwrap();
    }
}