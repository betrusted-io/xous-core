#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod std_tcpstream;
use std_tcpstream::*;
mod std_glue;
use std_glue::*;
mod std_udp;
use std_udp::*;
mod std_tcplistener;
use std_tcplistener::*;

use com::api::{ComIntSources, Ipv4Conf, NET_MTU};
use num_traits::*;

mod connection_manager;
mod device;

#[cfg(test)]
mod tests;
#[cfg(feature="btest")]
mod btests;

use std::collections::{BTreeMap, HashMap, BTreeSet};
use std::convert::TryInto;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message, CID, SID};
use xous_ipc::Buffer;

use byteorder::{ByteOrder, NetworkEndian};
use smoltcp::iface::{Interface, InterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::{Device, Medium};
use smoltcp::socket::{IcmpEndpoint, IcmpPacketMetadata, IcmpSocket, IcmpSocketBuffer};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, Ipv4Address, Ipv4Cidr, IpEndpoint};
use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr, Icmpv6Packet, Icmpv6Repr};
use crate::device::NetPhy;

use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU32, AtomicU16, Ordering};
use smoltcp::socket::{
    TcpSocket, TcpSocketBuffer, UdpPacketMetadata, UdpSocket, UdpSocketBuffer,
};
use smoltcp::iface::SocketHandle;
use smoltcp::time::{Duration, Instant};
use std::sync::Arc;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::cmp::Ordering as CmpOrdering;

// 0 indicates no address is currently assigned
pub static IPV4_ADDRESS: AtomicU32 = AtomicU32::new(0);
// stash the MAC address for inserstion as a loopback target. Coded as big-end bytes.
pub static MAC_ADDRESS_LSB: AtomicU32 = AtomicU32::new(0);
pub static MAC_ADDRESS_MSB: AtomicU16 = AtomicU16::new(0);

const PING_DEFAULT_TIMEOUT_MS: u32 = 10_000;
const MAX_DELAY_THREADS: u32 = 9;
const PING_IDENT: u16 = 0x22b;

fn set_ipv4_addr<DeviceT>(iface: &mut Interface<'_, DeviceT>, cidr: Ipv4Cidr)
where
    DeviceT: for<'d> Device<'d>,
{
    iface.update_ip_addrs(|addrs| {
        let dest = addrs
            .iter_mut()
            .next()
            .expect("trouble updating ipv4 addresses in routing table");
        if *dest.address().as_bytes() != [127, 0, 0, 1] {
            log::info!("updating to address {:?}", cidr);
            *dest = IpCidr::Ipv4(cidr);
        } else {
            log::info!("not updating loopback address");
        }
    });
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum WaitOp {
    WaitMs,
    PollAt,
    Quit,
}

/// PingConnection can return a Scalar: because of the simplicity of the return data
/// we give implementors the option to unpack the Scalar themselves within the main loop
/// of their event handler, *or* they can create a dedicated server that handles the return
/// code.
///
/// Unpacking the Scalar type is more efficient, but essentially requires a connection
/// to their private main loop server connection for the message to arrive, brokered via
/// xous-names. This can create a potential security concern, as the "unclaimed" connection
/// could be abused by a malicious process, which would have access to all of the dispatchable
/// opcodes of the main loop through that connection.
///
/// Thus, for security-sensitive processes, it is recommended that those create a single-purpose
/// server ID and broker the connection through that mechanism.
#[derive(Hash, PartialEq, Eq)]
pub struct PingConnection {
    remote: IpAddress,
    cid: CID,
    retop: usize,
}

#[derive(Debug)]
struct WaitingSocket {
    env: xous::MessageEnvelope,
    handle: SocketHandle,
    expiry: Option<NonZeroU64>,
}

struct AcceptingSocket {
    env: xous::MessageEnvelope,
    handle: SocketHandle,
    fd: usize,
}

pub struct UdpStdState {
    pub msg: xous::MessageEnvelope,
    pub handle: SocketHandle,
    pub expiry: Option<u64>,
}

pub struct Wakeup {
    pub tx_index: usize,
    pub time: u64,
}
impl Ord for Wakeup {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.time.cmp(&other.time)
    }
}
impl PartialOrd for Wakeup {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Wakeup {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl Eq for Wakeup {}
pub struct WorkerState {
    pub tx: Sender::<u64>,
    pub is_busy: bool,
    pub time_replica: u64, // this is just to help with debugging, nothing else
}

fn set_com_ints(com_int_list: &mut Vec<ComIntSources>) {
    com_int_list.clear();
    com_int_list.push(ComIntSources::WlanIpConfigUpdate);
    com_int_list.push(ComIntSources::WlanRxReady);
    com_int_list.push(ComIntSources::BatteryCritical);
    com_int_list.push(ComIntSources::Connect);
    com_int_list.push(ComIntSources::Disconnect);
    com_int_list.push(ComIntSources::WlanSsidScanUpdate);
    com_int_list.push(ComIntSources::WlanSsidScanFinished);
    com_int_list.push(ComIntSources::WfxErr);
    com_int_list.push(ComIntSources::Invalid);
}

fn setup_icmp(iface: &mut Interface::<NetPhy>) -> SocketHandle {
    // ping storage
    // up to four concurrent pings in the queue
    let icmp_rx_buffer = IcmpSocketBuffer::new(
        vec![
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
        ],
        vec![0; 1024],
    );
    let icmp_tx_buffer = IcmpSocketBuffer::new(
        vec![
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
            IcmpPacketMetadata::EMPTY,
        ],
        vec![0; 1024],
    );
    let icmp_socket = IcmpSocket::new(icmp_rx_buffer, icmp_tx_buffer);
    let icmp_handle = iface.add_socket(icmp_socket);
    let icmp_socket = iface.get_socket::<IcmpSocket>(icmp_handle);
    icmp_socket
        .bind(IcmpEndpoint::Ident(PING_IDENT))
        .expect("couldn't bind to icmp socket");
    icmp_handle
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let net_sid = xns
        .register_name(api::SERVER_NAME_NET, None)
        .expect("can't register server");
    let net_conn = xous::connect(net_sid).unwrap();
    log::trace!("registered with NS -- {:?}", net_sid);

    // bring the EC into a sane state for the network -- that is, reset the EC
    let mut llio = llio::Llio::new(&xns);
    let com = com::Com::new(&xns).unwrap();
    let timer = ticktimer_server::Ticktimer::new().unwrap();

    // we need a trng for port numbers
    let trng = trng::Trng::new(&xns).unwrap();

    // hook the COM interrupt listener
    let net_cid = xous::connect(net_sid).unwrap();
    llio.hook_com_event_callback(Opcode::ComInterrupt.to_u32().unwrap(), net_cid)
        .unwrap();
    llio.com_event_enable(true).unwrap();
    // setup the interrupt masks
    let mut com_int_list: Vec<ComIntSources> = vec![];
    com.ints_get_active(&mut com_int_list).ok();
    log::debug!("COM initial pending interrupts: {:?}", com_int_list);
    set_com_ints(&mut com_int_list);
    com.ints_enable(&com_int_list);
    com_int_list.clear();
    com.ints_get_active(&mut com_int_list).ok();
    log::debug!("COM pending interrupts after enabling: {:?}", com_int_list);
    let mut net_config: Option<Ipv4Conf> = None;

    // ------------- libstd variant -----------
    // Each process keeps track of its own sockets. These are kept in a Vec. When a handle
    // is destroyed, it is turned into a `None`.
    let mut process_sockets: HashMap<Option<xous::PID>, Vec<Option<SocketHandle>>> = HashMap::new();

    // When a TCP client issues a Receive request, it will get placed here while the packet data
    // is being accumulated.
    let mut tcp_rx_waiting: Vec<Option<WaitingSocket>> = Vec::new();
    let mut tcp_peek_waiting: Vec<Option<WaitingSocket>> = Vec::new();

    // When a client issues a Send request, it will get placed here while the packet data
    // is being accumulated.
    let mut tcp_tx_waiting: Vec<Option<WaitingSocket>> = Vec::new();

    // socket handles waiting for writes to flush on close (transitions to sending FIN)
    let mut tcp_tx_closing: Vec<(SocketHandle, xous::MessageSender)> = Vec::new();

    // socket handles waiting to enter the closed state
    let mut tcp_tx_wait_fin: Vec<(SocketHandle, xous::MessageSender, u32)> = Vec::new();

    // socket handles corresponding to servers that could be closed by clients
    let mut tcp_server_remote_close_poll: Vec<SocketHandle> = Vec::new();

    // When a client issues a Connect request, it will get placed here while the connection is
    // being established.
    let mut tcp_connect_waiting: Vec<
        Option<(
            xous::MessageEnvelope,
            SocketHandle,
            u16, /* fd */
            u16, /* local_port */
            u16, /* remote_port */
        )>,
    > = Vec::new();

    // When a client issues an Accept request, it gets placed here for later processing.
    let mut tcp_accept_waiting: Vec<Option<AcceptingSocket>> = Vec::new();

    // When a UDP client opens a socket, an entry is automatically created here to accumulate
    // incoming UDP socket data.
    let mut udp_rx_waiting: Vec<Option<UdpStdState>> = Vec::new();

    // --------------- other link storage -------------
    let neighbor_cache = NeighborCache::new(BTreeMap::new());
    let ip_addrs = [
        IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0),
        IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8)
    ];
    let routes = Routes::new(BTreeMap::new());

    // build the device
    let hw_config = match com.wlan_get_config() {
        Ok(config) => config,
        Err(e) => {
            log::error!("Something is wrong with the EC, got {:?} when requesting a MAC address. Trying our best to bodge through it.", e);
            Ipv4Conf {
                dhcp: com_rs::DhcpState::Invalid,
                mac: [2, 2, 4, 5, 6, 2],
                addr: [169, 254, 0, 2], // link local address
                gtwy: [169, 254, 0, 1], // something bogus
                mask: [255, 255, 0, 0,],
                dns1: [1, 1, 1, 1],
                dns2: [8, 8, 8, 8],
            }
        }
    };
    log::debug!("My MAC address is: {:x?}", hw_config.mac);
    MAC_ADDRESS_LSB.store(u32::from_be_bytes(hw_config.mac[2..6].try_into().unwrap()), Ordering::SeqCst);
    MAC_ADDRESS_MSB.store(u16::from_be_bytes(hw_config.mac[0..2].try_into().unwrap()), Ordering::SeqCst);
    let device = device::NetPhy::new(&xns, net_cid);
    // needed by ICMP to determine if we should compute checksums
    let device_caps = device.capabilities();
    let medium = device.capabilities().medium;
    let mut builder = InterfaceBuilder::new(device, vec![])
        .ip_addrs(ip_addrs)
        .routes(routes);
    if medium == Medium::Ethernet {
        builder = builder
            .hardware_addr(EthernetAddress::from_bytes(&hw_config.mac).into())
            .neighbor_cache(neighbor_cache);
    }
    let mut iface = builder.finalize();

    // ------------- native variant -----------
    let icmp_handle = setup_icmp(&mut iface);
    let mut seq: u16 = 0;
    // this record stores the origin time + IP address of the outgoing ping sequence number
    let mut ping_destinations = HashMap::<PingConnection, HashMap<u16, u64>>::new();
    let mut ping_timeout_ms = PING_DEFAULT_TIMEOUT_MS;

    // DNS hooks - the DNS server can ask the Net crate to tickle it when IP configs change using these hooks
    // Currently, we assume there is only one DNS server in Xous. I suppose you could
    // upgrade the code to handle multiple DNS servers, but...why???
    // ... nevermind, someone will invent a $reason because there was never a shiny
    // new feature that a coder didn't love and *had* to have *right now*.
    let mut dns_ipv4_hook = XousScalarEndpoint::new();
    let mut dns_ipv6_hook = XousScalarEndpoint::new();
    let mut dns_allclear_hook = XousScalarEndpoint::new();

    // wakeup polling management - kick off worker threads to wake up a poll in the future for certain rx events required by smoltcp
    // this is not reset on connection reset, because the stale timers are still
    // running and there is no way to reset them mid-run.
    let mut wakeup_time: BTreeSet<Wakeup> = BTreeSet::new();
    let mut wakeup_workers = Vec::<WorkerState>::new();
    for _ in 0..MAX_DELAY_THREADS {
        let (tx, rx) = channel();
        wakeup_workers.push(WorkerState {
            tx,
            is_busy: false,
            time_replica: 0,
        });
        thread::spawn({
            let parent_conn = net_conn.clone();
            move || {
                let tt = ticktimer_server::Ticktimer::new().unwrap();
                loop {
                    let target = rx.recv().unwrap();
                    let now = tt.elapsed_ms();
                    // arbitrary time can pass between the target issuance and receive
                    let wakeup = if now >= target {
                        0
                    } else {
                        target - now
                    };
                    if wakeup > 100_000 {
                        log::warn!("long wakeup detected: {}", wakeup);
                    }
                    if wakeup != 0 { // only issue the sleep if the wakeup time is non-zero
                        tt.sleep_ms(wakeup as usize).unwrap();
                    }
                    xous::try_send_message(
                        parent_conn,
                        Message::new_scalar(
                            Opcode::NetPump.to_usize().unwrap(),
                            (target >> 32) as usize,
                            (target & 0xffff_ffff) as usize,
                            0,
                            0,
                        ),
                    )
                    .ok();
                }
            }
        });
    }

    log::trace!("ready to accept requests");
    // register a suspend/resume listener
    let sr_cid = xous::connect(net_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(
        Some(susres::SuspendOrder::Early),
        &xns,
        api::Opcode::SuspendResume as u32,
        sr_cid,
    )
    .expect("couldn't create suspend/resume object");

    // kick off the connection manager thread
    let cm_sid = xous::create_server().expect("couldn't create connection manager server");
    let cm_cid = xous::connect(cm_sid).unwrap();
    let activity_interval = Arc::new(AtomicU32::new(0));
    #[cfg(not(feature = "renode-minimal"))]
    thread::spawn({
        let activity_interval = activity_interval.clone();
        move || {
            connection_manager::connection_manager(cm_sid, activity_interval);
        }
    });

    let mut cid_to_disconnect: Option<CID> = None;
    loop {
        let mut msg = xous::receive_message(net_sid).unwrap();
        if let Some(dc_cid) = cid_to_disconnect.take() {
            // disconnect previous loop iter's connection after d/c OK response was sent
            unsafe {
                match xous::disconnect(dc_cid) {
                    Ok(_) => {}
                    Err(xous::Error::ServerNotFound) => {
                        log::trace!("Disconnect returned the expected error code for a remote that has been destroyed.")
                    }
                    Err(e) => {
                        log::error!(
                            "Attempt to de-allocate CID to destroyed server met with error: {:?}",
                            e
                        );
                    }
                }
            }
        }
        let op = FromPrimitive::from_usize(msg.body.id() & 0x7fff);
        let nonblocking = (msg.body.id() & NONBLOCKING_FLAG) != 0;
        log::debug!("{:?}", op);
        match op {
            Some(Opcode::Ping) => {
                log::debug!("Ping");
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut pkt = buf.to_original::<NetPingPacket, _>().unwrap();
                let socket = iface.get_socket::<IcmpSocket>(icmp_handle);
                if socket.can_send() {
                    log::debug!("sending ping to {:?}", pkt.endpoint);
                    let remote = IpAddress::from(pkt.endpoint);
                    // we take advantage of the fact that the same CID is always returned for repeated connect requests to the same SID.
                    let cid = match pkt.server {
                        XousServerId::PrivateSid(sid) => {
                            match xous::connect(SID::from_array(sid)) {
                                Ok(cid) => cid,
                                Err(e) => {
                                    log::error!("Ping request with single-use callback SID is invalid. Aborting request. {:?}",e);
                                    continue;
                                }
                            }
                        }
                        XousServerId::ServerName(name) => {
                            match xns.request_connection(name.to_str()) {
                                Ok(cid) => cid,
                                Err(e) => {
                                    log::error!("Ping request received, but callback name '{}' is invalid. Aborting request. {:?}", name, e);
                                    continue;
                                }
                            }
                        }
                    };
                    // this structure can be a HashMap key because it "should" be invariant across well-formed ping requests
                    let conn = PingConnection {
                        remote,
                        cid,
                        retop: pkt.return_opcode,
                    };
                    log::trace!(
                        "ping conn info: remote {:?} / cid: {} / retp: {}",
                        remote,
                        cid,
                        pkt.return_opcode
                    );
                    // this code will guarantee the sequence number goes up, but if multiple concurrent
                    // pings are in progress, they may not be directly in sequence. This is OK.
                    let now = timer.elapsed_ms();
                    if let Some(queue) = ping_destinations.get_mut(&conn) {
                        queue.insert(seq, now);
                    } else {
                        let mut new_queue = HashMap::<u16, u64>::new();
                        new_queue.insert(seq, now);
                        ping_destinations.insert(conn, new_queue);
                    };

                    // now emit the actual packet
                    let mut echo_payload = [0xffu8; 40];
                    NetworkEndian::write_i64(&mut echo_payload, now as i64);
                    match remote {
                        IpAddress::Ipv4(_) => {
                            let icmp_repr = Icmpv4Repr::EchoRequest {
                                ident: PING_IDENT,
                                seq_no: seq,
                                data: &echo_payload,
                            };
                            let icmp_payload = socket.send(icmp_repr.buffer_len(), remote).unwrap();
                            let mut icmp_packet = Icmpv4Packet::new_unchecked(icmp_payload);
                            icmp_repr.emit(&mut icmp_packet, &device_caps.checksum);
                        }
                        IpAddress::Ipv6(_) => {
                            // not sure if this is a valid thing to do, to just assign the source some number like this??
                            let src_ipv6 = IpAddress::v6(0xfdaa, 0, 0, 0, 0, 0, 0, 1);
                            let icmp_repr = Icmpv6Repr::EchoRequest {
                                ident: PING_IDENT,
                                seq_no: seq,
                                data: &echo_payload,
                            };
                            let icmp_payload = socket.send(icmp_repr.buffer_len(), remote).unwrap();
                            let mut icmp_packet = Icmpv6Packet::new_unchecked(icmp_payload);
                            icmp_repr.emit(
                                &src_ipv6,
                                &remote,
                                &mut icmp_packet,
                                &device_caps.checksum,
                            );
                        }
                        _ => unimplemented!(),
                    }
                    seq += 1;
                    // fire off a Pump to get the stack to actually transmit the ping; this call merely queues it for sending
                    xous::try_send_message(
                        net_conn,
                        Message::new_scalar(Opcode::NetPump.to_usize().unwrap(), 0, 0, 0, 0),
                    )
                    .ok();
                    pkt.sent_ok = Some(true);
                } else {
                    pkt.sent_ok = Some(false);
                }
                buf.replace(pkt)
                    .expect("Xous couldn't issue response to Ping request");
            }
            Some(Opcode::PingSetTtl) => msg_scalar_unpack!(msg, ttl, _, _, _, {
                let checked_ttl = if ttl > 255 { 255 as u8 } else { ttl as u8 };
                let socket = iface.get_socket::<IcmpSocket>(icmp_handle);
                socket.set_hop_limit(Some(checked_ttl));
            }),
            Some(Opcode::PingGetTtl) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let socket = iface.get_socket::<IcmpSocket>(icmp_handle);
                let checked_ttl = if let Some(ttl) = socket.hop_limit() {
                    ttl
                } else {
                    64 // because this is the default according to the smoltcp source code
                };
                xous::return_scalar(msg.sender, checked_ttl as usize).unwrap();
            }),
            Some(Opcode::PingSetTimeout) => msg_scalar_unpack!(msg, to, _, _, _, {
                ping_timeout_ms = to as u32;
            }),
            Some(Opcode::PingGetTimeout) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, ping_timeout_ms as usize).unwrap();
            }),
            Some(Opcode::DnsHookAddIpv4) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let hook = buf.to_original::<XousPrivateServerHook, _>().unwrap();
                if dns_ipv4_hook.is_set() {
                    buf.replace(NetMemResponse::AlreadyUsed).unwrap();
                } else {
                    dns_ipv4_hook.set(
                        xous::connect(SID::from_array(hook.one_time_sid)).unwrap(),
                        hook.op,
                        hook.args,
                    );
                    buf.replace(NetMemResponse::Ok).unwrap();
                }
            }
            Some(Opcode::DnsHookAddIpv6) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let hook = buf.to_original::<XousPrivateServerHook, _>().unwrap();
                if dns_ipv6_hook.is_set() {
                    buf.replace(NetMemResponse::AlreadyUsed).unwrap();
                } else {
                    dns_ipv6_hook.set(
                        xous::connect(SID::from_array(hook.one_time_sid)).unwrap(),
                        hook.op,
                        hook.args,
                    );
                    buf.replace(NetMemResponse::Ok).unwrap();
                }
            }
            Some(Opcode::DnsHookAllClear) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let hook = buf.to_original::<XousPrivateServerHook, _>().unwrap();
                if dns_allclear_hook.is_set() {
                    buf.replace(NetMemResponse::AlreadyUsed).unwrap();
                } else {
                    dns_allclear_hook.set(
                        xous::connect(SID::from_array(hook.one_time_sid)).unwrap(),
                        hook.op,
                        hook.args,
                    );
                    buf.replace(NetMemResponse::Ok).unwrap();
                }
            }
            Some(Opcode::DnsUnhookAll) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                dns_ipv4_hook.clear();
                dns_ipv6_hook.clear();
                dns_allclear_hook.clear();
                xous::return_scalar(msg.sender, 1).expect("couldn't ack unhook");
            }),

            Some(Opcode::StdTcpConnect) => {
                // Pick a random local port using the system's TRNG
                let local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
                let pid = msg.sender.pid();

                std_tcp_connect(
                    msg,
                    local_port,
                    &mut iface,
                    &mut tcp_connect_waiting,
                    process_sockets.entry(pid).or_default(),
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpTx) => {
                log::debug!("StdTcpTx");
                let pid = msg.sender.pid();
                std_tcp_tx(
                    msg,
                    &timer,
                    &mut iface,
                    &mut tcp_tx_waiting,
                    process_sockets.entry(pid).or_default(),
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpPeek) => {
                log::debug!("StdTcpPeek");
                let pid = msg.sender.pid();
                std_tcp_peek(
                    msg,
                    &timer,
                    &mut iface,
                    process_sockets.entry(pid).or_default(),
                    &mut tcp_peek_waiting,
                    nonblocking,
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpRx) => {
                log::debug!("StdTcpRx");
                let pid = msg.sender.pid();
                std_tcp_rx(
                    msg,
                    &timer,
                    &mut iface,
                    &mut tcp_rx_waiting,
                    process_sockets.entry(pid).or_default(),
                    nonblocking,
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpClose) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                let handle = if let Some(connection) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    if let Some(connection) = connection.take() {
                        connection
                    } else {
                        respond_with_error(msg, NetError::Invalid);
                        continue;
                    }
                } else {
                    respond_with_error(msg, NetError::Invalid);
                    continue;
                };
                let socket = iface.get_socket::<TcpSocket>(handle);
                log::debug!("StdTcpClose {:?}", socket.local_endpoint());
                if !std_tcp_can_close(&tcp_tx_waiting, handle) {
                    log::trace!("def"); // these are short because the extra delay of a long message affects the computation
                    tcp_tx_closing.push((handle, msg.sender));
                } else {
                    if socket.may_send() && socket.send_queue() == 0 {
                        log::trace!("imm");
                        socket.close();
                        tcp_tx_wait_fin.push((handle, msg.sender, 0));
                        //log::info!("EARLY CLOSE");
                        //xous::return_scalar(msg.sender, 0).ok(); // ack early so we don't block other processes waiting to close
                    } else {
                        log::trace!("def2");
                        tcp_tx_closing.push((handle, msg.sender));
                    }
                }
            }

            Some(Opcode::StdTcpStreamShutdown) => {
                log::debug!("StdTcpStreamShutdown");
                // Only work with blockingscalar messages
                if !msg.body.is_blocking() || msg.body.has_memory() {
                    respond_with_error(msg, NetError::LibraryError);
                    continue;
                }

                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                let shutdown_code = msg.body.scalar_message().unwrap().arg1;
                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get(connection_idx)
                {
                    if (shutdown_code & 1) != 0 { // read shutdown
                        // search for the handle in the rxwaiting set
                        for rx_waiter in tcp_rx_waiting.iter_mut() {
                            let WaitingSocket {
                                env: mut msg,
                                handle,
                                expiry: _,
                            } = match rx_waiter {
                                &mut None => continue,
                                Some(s) => {
                                    if s.handle == *connection {
                                        rx_waiter.take().unwrap() // removes the message from the waiting queue
                                    } else {
                                        continue
                                    }
                                }
                            };
                            // if we got here, we found a message that needs to be aborted
                            log::info!("TcpShutdown: aborting rx waiting handle: {:?}", handle);
                            match msg.body.memory_message_mut() {
                                Some(body) => {
                                    // u32::MAX indicates a zero-length receive
                                    body.valid = xous::MemorySize::new(u32::MAX as usize);
                                },
                                None => {
                                    respond_with_error(msg, NetError::LibraryError);
                                }
                            }
                            // in theory, there should be no more matching handles as they should be all unique, so we can abort the search.
                            break;
                        }
                        for rx_waiter in tcp_peek_waiting.iter_mut() {
                            let WaitingSocket {
                                env: mut msg,
                                handle,
                                expiry: _,
                            } = match rx_waiter {
                                &mut None => continue,
                                Some(s) => {
                                    if s.handle == *connection {
                                        rx_waiter.take().unwrap() // removes the message from the waiting queue
                                    } else {
                                        continue
                                    }
                                }
                            };
                            // if we got here, we found a message that needs to be aborted
                            log::info!("TcpShutdown: aborting peek waiting handle: {:?}", handle);
                            match msg.body.memory_message_mut() {
                                Some(body) => {
                                    // u32::MAX indicates a zero-length receive
                                    body.valid = xous::MemorySize::new(u32::MAX as usize);
                                },
                                None => {
                                    respond_with_error(msg, NetError::LibraryError);
                                }
                            }
                            // in theory, there should be no more matching handles as they should be all unique, so we can abort the search.
                            break;
                        }
                    }
                    if (shutdown_code & 2) != 0 { // write shutdown
                        // search for the handle in the txwaiting set
                        for tx_waiter in tcp_tx_waiting.iter_mut() {
                            let WaitingSocket {
                                env: mut msg,
                                handle,
                                expiry: _,
                            } = match tx_waiter {
                                &mut None => continue,
                                Some(s) => {
                                    if s.handle == *connection {
                                        tx_waiter.take().unwrap() // removes the message from the waiting queue
                                    } else {
                                        continue
                                    }
                                }
                            };
                            // if we got here, we found a message that needs to be aborted
                            log::info!("TcpShutdown: aborting tx waiting handle: {:?}", handle);
                            match msg.body.memory_message_mut() {
                                Some(body) => {
                                    // u32::MAX indicates a zero-length receive
                                    body.valid = xous::MemorySize::new(u32::MAX as usize);
                                    let response_data = body.buf.as_slice_mut::<u32>();
                                    response_data[0] = 0;
                                    response_data[1] = 0;
                                },
                                None => {
                                    respond_with_error(msg, NetError::LibraryError);
                                }
                            }
                            // in theory, there should be no more matching handles as they should be all unique, so we can abort the search.
                            break;
                        }
                    }
                }

                // unblock the sender
                xous::return_scalar(msg.sender, 1).ok();
                // pump the rx to process any shutdowns
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpListen) => {
                let pid = msg.sender.pid();

                std_tcp_listen(
                    msg,
                    &mut iface,
                    process_sockets.entry(pid).or_default(),
                    &trng,
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdTcpAccept) => {
                let pid = msg.sender.pid();

                std_tcp_accept(
                    msg,
                    &mut iface,
                    &mut tcp_accept_waiting,
                    &mut tcp_server_remote_close_poll,
                    process_sockets.entry(pid).or_default(),
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdGetAddress) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                let body = match msg.body.memory_message_mut() {
                    Some(body) => body,
                    None => {
                        respond_with_error(msg, NetError::LibraryError);
                        continue;
                    }
                };

                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    let socket = iface.get_socket::<TcpSocket>(*connection);
                    body.valid = xous::MemorySize::new(
                        write_address(socket.local_endpoint().addr, body.buf.as_slice_mut())
                            .unwrap_or_default(),
                    );
                } else {
                    respond_with_error(msg, NetError::Invalid);
                }
            }

            Some(Opcode::StdGetTtl) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                // Only work with blockingscalar messages
                if !msg.body.is_blocking() || msg.body.has_memory() {
                    respond_with_error(msg, NetError::LibraryError);
                    continue;
                }

                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    let args = msg.body.scalar_message().unwrap();
                    let limit = if args.arg4 == 1 {
                        let socket = iface.get_socket::<UdpSocket>(*connection);
                        socket.hop_limit().unwrap_or(64) as usize
                    } else {
                        let socket = iface.get_socket::<TcpSocket>(*connection);
                        socket.hop_limit().unwrap_or(64) as usize
                    };
                    xous::return_scalar(
                        msg.sender,
                        limit,
                    )
                    .ok();
                } else {
                    respond_with_error(msg, NetError::Invalid);
                    continue;
                }
            }

            Some(Opcode::StdSetTtl) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                // Only work with blockingscalar messages
                if !msg.body.is_blocking() || msg.body.has_memory() {
                    respond_with_error(msg, NetError::LibraryError);
                    continue;
                }

                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    let args = msg.body.scalar_message().unwrap();
                    let hop_limit = if (args.arg1 == 0) || (args.arg1 > 255) {
                        None
                    } else {
                        Some(args.arg1 as u8)
                    };
                    if args.arg4 == 1 {
                        let socket = iface.get_socket::<UdpSocket>(*connection);
                        socket.set_hop_limit(hop_limit);
                    } else {
                        let socket = iface.get_socket::<TcpSocket>(*connection);
                        socket.set_hop_limit(hop_limit);
                    }
                    xous::return_scalar(msg.sender, 0).ok();
                } else {
                    respond_with_error(msg, NetError::Invalid);
                    continue;
                }
            }

            Some(Opcode::StdGetNodelay) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                // Only work with blockingscalar messages
                if !msg.body.is_blocking() || msg.body.has_memory() {
                    respond_with_error(msg, NetError::LibraryError);
                    continue;
                }

                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    let socket = iface.get_socket::<TcpSocket>(*connection);
                    // this is actually broken right now; it's fixed in https://github.com/smoltcp-rs/smoltcp/commit/8090469c2a1456b87a3e277e5b2f2e34e4f7555b
                    // not worth forking over this, though. We'll leave this as an erratum.
                    let nagle_enabled = socket.nagle_enabled().is_some(); // this return type will turn to bool on the next release.
                    log::warn!("Issue #210: nagle value read is always incorrect. Setting works, however. Returning nagle value of {}", nagle_enabled);
                    let no_delay = !nagle_enabled;
                    xous::return_scalar(
                        msg.sender,
                        if no_delay {
                            1
                        } else {
                            0
                        },
                    )
                    .ok();
                } else {
                    respond_with_error(msg, NetError::Invalid);
                }
            }

            Some(Opcode::StdSetNodelay) => {
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                // Only work with blockingscalar messages
                if !msg.body.is_blocking() || msg.body.has_memory() {
                    respond_with_error(msg, NetError::LibraryError);
                    continue;
                }

                if let Some(Some(connection)) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    let socket = iface.get_socket::<TcpSocket>(*connection);
                    let args = msg.body.scalar_message().unwrap();
                    let no_delay = args.arg1 != 0;
                    log::warn!("Setting nagle to {}, see issue #210 about readback!", !no_delay);
                    socket.set_nagle_enabled(!no_delay);
                    xous::return_scalar(msg.sender, 0).ok();
                } else {
                    respond_with_error(msg, NetError::Invalid);
                };
            }

            Some(Opcode::StdUdpBind) => {
                log::debug!("StdUdpBind");
                let pid = msg.sender.pid();
                std_udp_bind(
                    msg,
                    &mut iface,
                    process_sockets.entry(pid).or_default(),
                );
            }

            Some(Opcode::StdUdpRx) => {
                log::debug!("StdUdpRx");
                let pid = msg.sender.pid();
                std_udp_rx(
                    msg,
                    &timer,
                    &mut iface,
                    &mut udp_rx_waiting,
                    process_sockets.entry(pid).or_default(),
                );
            }

            Some(Opcode::StdUdpTx) => {
                log::debug!("StdUdpTx");
                let pid = msg.sender.pid();
                std_udp_tx(
                    msg,
                    &mut iface,
                    process_sockets.entry(pid).or_default(),
                );
                xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0, 0, 0, 0
                    ),
                ).ok();
            }

            Some(Opcode::StdUdpClose) => {
                log::debug!("StdUdpClose");
                let pid = msg.sender.pid();
                let connection_idx = msg.body.id() >> 16;
                let handle = if let Some(connection) = process_sockets
                    .entry(pid)
                    .or_default()
                    .get_mut(connection_idx)
                {
                    if let Some(connection) = connection.take() {
                        connection
                    } else {
                        std_failure(msg, NetError::Invalid);
                        continue;
                    }
                } else {
                    std_failure(msg, NetError::Invalid);
                    continue;
                };
                iface.get_socket::<UdpSocket>(handle).close();
                iface.remove_socket(handle);
                if let Some(response) = msg.body.memory_message_mut() {
                    response.buf.as_slice_mut::<u8>()[0] = 0;
                } else if !msg.body.has_memory() && msg.body.is_blocking() {
                    xous::return_scalar(msg.sender, 0).ok();
                }
            }

            Some(Opcode::ComInterrupt) => {
                com_int_list.clear();
                match com.ints_get_active(&mut com_int_list) {
                    Ok((maybe_rxlen, ints, raw_rxlen)) => {
                        log::debug!("COM got interrupts: {:?}, {:?}", com_int_list, maybe_rxlen);
                        // forward the interrupt to the connection manager as well
                        match xous::try_send_message(
                            cm_cid,
                            Message::new_scalar(
                                connection_manager::ConnectionManagerOpcode::ComInt
                                    .to_usize()
                                    .unwrap(),
                                ints,
                                raw_rxlen,
                                0,
                                0,
                            ),
                        ) {
                            Ok(_) => {}
                            Err(xous::Error::ServerQueueFull) => {
                                log::warn!("Our net queue runneth over, interrupts were dropped.");
                            }
                            Err(e) => {
                                log::error!("Unhandled error forwarding ComInt to the connection manager: {:?}", e);
                            }
                        };
                        for &pending in com_int_list.iter() {
                            if pending == ComIntSources::Invalid {
                                log::error!("COM interrupt vector had an error, ignoring event.");
                                continue;
                            }
                        }
                        for &pending in com_int_list.iter() {
                            match pending {
                                ComIntSources::BatteryCritical => {
                                    log::warn!("Battery is critical! TODO: go into SHIP mode");
                                }
                                ComIntSources::WlanIpConfigUpdate => {
                                    // right now the WLAN implementation only does IPV4. So IPV6 compatibility ends here.
                                    // if IPV6 gets added to the EC/COM bus, ideally this is one of a couple spots in Xous that needs a tweak.
                                    let config = match com
                                    .wlan_get_config() {
                                        Ok(config) => config,
                                        Err(e) => {
                                            log::error!("WLAN config interrupt was bogus. EC is probably updating? Ignoring. Error: {:?}", e);
                                            continue;
                                        }
                                    };
                                    log::info!("Network config acquired: {:?}", config);
                                    log::info!("{}NET.OK,{:?},{}",
                                        xous::BOOKEND_START,
                                        std::net::IpAddr::from(config.addr),
                                        xous::BOOKEND_END);
                                    net_config = Some(config);
                                    // update a static variable that tracks this, useful for e.g. UDP bind address checking
                                    IPV4_ADDRESS.store(u32::from_be_bytes(config.addr), Ordering::SeqCst);

                                    // note: ARP cache is stale. Maybe that's ok?

                                    let ip_addr = Ipv4Cidr::new(
                                        Ipv4Address::new(
                                            config.addr[0],
                                            config.addr[1],
                                            config.addr[2],
                                            config.addr[3],
                                        ),
                                        24,
                                    );
                                    set_ipv4_addr(&mut iface, ip_addr);
                                    let default_v4_gw = Ipv4Address::new(
                                        config.gtwy[0],
                                        config.gtwy[1],
                                        config.gtwy[2],
                                        config.gtwy[3],
                                    );

                                    // reset the default route, in case it has changed
                                    iface.routes_mut().remove_default_ipv4_route();
                                    match iface.routes_mut().add_default_ipv4_route(default_v4_gw) {
                                        Ok(route) => log::info!(
                                            "routing table updated successfully [{:?}]",
                                            route
                                        ),
                                        Err(e) => log::error!("routing table update error: {}", e),
                                    }
                                    dns_allclear_hook.notify();
                                    dns_ipv4_hook.notify_custom_args([
                                        Some(u32::from_be_bytes(config.dns1)),
                                        None,
                                        None,
                                        None,
                                    ]);
                                    // the current implementation always returns 0.0.0.0 as the second dns,
                                    // ignore this if that's what we've got; otherwise, pass it on.
                                    if config.dns2 != [0, 0, 0, 0] {
                                        dns_ipv4_hook.notify_custom_args([
                                            Some(u32::from_be_bytes(config.dns2)),
                                            None,
                                            None,
                                            None,
                                        ]);
                                    }
                                }
                                ComIntSources::WlanRxReady => {
                                    activity_interval.store(0, Ordering::Relaxed); // reset the activity interval to 0
                                    if let Some(_config) = net_config {
                                        if let Some(rxlen) = maybe_rxlen {
                                            match iface.device_mut().push_rx_avail(rxlen) {
                                                None => {} //log::info!("pushed {} bytes avail to iface", rxlen),
                                                Some(_) => log::warn!("Got more packets, but smoltcp didn't drain them in time"),
                                            }
                                            match xous::try_send_message(
                                                net_conn,
                                                Message::new_scalar(
                                                    Opcode::NetPump.to_usize().unwrap(),
                                                    0,
                                                    0,
                                                    0,
                                                    0,
                                                ),
                                            ) {
                                                Ok(_) => {}
                                                Err(xous::Error::ServerQueueFull) => {
                                                    log::warn!("Our net queue runneth over, packets will be dropped.");
                                                }
                                                Err(e) => {
                                                    log::error!("Unhandled error sending NetPump to self: {:?}", e);
                                                }
                                            }
                                        } else {
                                            log::error!("Got RxReady interrupt but no packet length specified!");
                                        }
                                    }
                                }
                                ComIntSources::Invalid => {
                                    com.ints_ack(&com_int_list); // ack everything that's pending
                                    // re-enable the interrupts as we intended
                                    let mut ena_list: Vec<ComIntSources> = vec![];
                                    set_com_ints(&mut ena_list);
                                    com.ints_enable(&ena_list);
                                }
                                _ => {
                                    log::debug!("Unhandled: {:?}", pending);
                                }
                            }
                        }
                        com.ints_ack(&com_int_list);
                    }
                    Err(xous::Error::Timeout) => {
                        log::warn!("Interrupt fetch from COM timed out.");
                        // bread crumb: this is a "normal" error to throw when the EC is being reset,
                        // or when it is handling the reset of the wifi subsystem, so it's not fatal.
                        // However, if we see this repeatedly, it might be a good idea to add some sort
                        // of event counter to log the number of times we've seen this consecutively and
                        // if it's too much, issue a reset to the EC.

                        // refresh the interrupt list to the EC, just in case it lost the prior list
                        timer.sleep_ms(1000).unwrap(); // a brief delay because if the EC wasn't responding before, it probably needs /some/ time before being able to handle this next message
                        set_com_ints(&mut com_int_list);
                        com.ints_enable(&com_int_list);
                        com_int_list.clear();
                    }
                    _ => {
                        // not fatal, just report it.
                        log::error!("Unhanlded error in COM interrupt fetch");
                    }
                }
            }
            Some(Opcode::LoopbackRx) => msg_scalar_unpack!(msg, _rxlen, _, _, _, {
                /*
                // the rx buf for loopback is different from the wlan interface
                // loopback uses an "infinite" internal buffer with its own length tracking, so we don't need
                // to track rxlen
                match iface.device_mut().push_rx_avail(rxlen as u16) {
                    None => {} //log::info!("pushed {} bytes avail to iface", rxlen),
                    Some(_) => log::warn!("Got more loopback packets, but smoltcp didn't drain them in time"),
                } */
                match xous::try_send_message(
                    net_conn,
                    Message::new_scalar(
                        Opcode::NetPump.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                ) {
                    Ok(_) => {}
                    Err(xous::Error::ServerQueueFull) => {
                        log::warn!("Our net queue runneth over, packets will be dropped.");
                    }
                    Err(e) => {
                        log::error!("Unhandled error sending NetPump to self: {:?}", e);
                    }
                }
            }),
            Some(Opcode::NetPump) => msg_scalar_unpack!(msg, wup_hi, wup_lo, _, _, {
                log::trace!("NetPump");
                // assume: if wup_hi/wup_lo == 0, then the wakeup is from a non-timer thread
                let wakeup: u64 = (wup_hi as u64) << 32 | (wup_lo as u64);
                if wakeup != 0 {
                    if let Some(wup) = wakeup_time.take(& Wakeup{tx_index: 0, time: wakeup}) {
                        wakeup_workers[wup.tx_index].is_busy = false;
                    } else {
                        log::warn!("wakeup time {} was not in the wakeup queue!", wakeup);
                    }
                    if wakeup_time.len() == 0 {
                        for (index, worker) in wakeup_workers.iter().enumerate() {
                            if worker.is_busy {
                                log::warn!("stranded worker at {}", index)
                            }
                        }
                    }
                    log::trace!("pending wakeups: {}", wakeup_time.len());
                }

                let now = timer.elapsed_ms();
                let timestamp = Instant::from_millis(now as i64);
                match iface.poll(timestamp) {
                    Ok(_) => {}
                    Err(e) => {
                        log::debug!("poll error: {}", e);
                    }
                }

                // Connect calls take time to establish. This block checks to see if connections
                // have been made and issues callbacks as necessary.
                log::trace!("pump: tcpconnect");
                for connection in tcp_connect_waiting.iter_mut() {
                    use smoltcp::socket::TcpState;
                    let socket;
                    let (env, _handle, fd, local_port, remote_port) = {
                        // If the connection is blank, or if it's still waiting to get
                        // connected, don't do anything.
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = iface.get_socket::<TcpSocket>(s.1);
                                log::debug!("connect state: {:?}", socket.state());
                                if socket.state() == TcpState::SynSent
                                    || socket.state() == TcpState::SynReceived
                                {
                                    continue;
                                }
                            }
                        }
                        connection.take().unwrap()
                    };

                    log::debug!("tcp state is {:?}", socket.state());
                    if socket.state() == TcpState::Established {
                        respond_with_connected(env, fd, local_port, remote_port);
                    } else {
                        respond_with_error(env, NetError::TimedOut);
                    }
                }

                // This block handles TCP Rx for libstd callers
                log::trace!("pump: tcp rx");
                for connection in tcp_rx_waiting.iter_mut() {
                    let socket;
                    let WaitingSocket {
                        mut env,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = iface.get_socket::<TcpSocket>(s.handle);
                                log::debug!("rx_state: {:?} {:?}", socket.state(), socket.local_endpoint());
                                if !socket.can_recv() {
                                    if let Some(trigger) = s.expiry {
                                        log::debug!("rxrcv {:?}", trigger.get());
                                        if trigger.get() < now {
                                            // timer expired
                                        } else {
                                            continue;
                                        }
                                    } else if socket.state() == smoltcp::socket::TcpState::CloseWait
                                    // this state added to handle the auto-close edge case on a remote hang-up
                                    || socket.state() == smoltcp::socket::TcpState::Closed {
                                        // stop waiting if we're in CloseWait, as we don't plan to transmit
                                    } else {
                                        continue;
                                    }
                                }
                            }
                        }
                        connection.take().unwrap()
                    };

                    // If it can't receive, then the only explanation was that it timed out
                    if !socket.can_recv() {
                        if socket.state() == smoltcp::socket::TcpState::CloseWait
                        // this state added to handle the auto-close edge case on a remote hang-up
                        || socket.state() == smoltcp::socket::TcpState::Closed {
                            log::debug!("rxrcv connection closed");
                            let body = env.body.memory_message_mut().unwrap();
                            log::debug!("rxrcv of {}", 0);
                            body.valid = xous::MemorySize::new(0);
                            body.offset = xous::MemoryAddress::new(1);
                            continue;
                        } else {
                            log::debug!("rxrcv timed out");
                            respond_with_error(env, NetError::TimedOut);
                            continue;
                        }
                    }

                    let body = env.body.memory_message_mut().unwrap();
                    let buflen = if let Some(valid) = body.valid {
                        valid.get()
                    } else {
                        0
                    };
                    match socket.recv_slice(&mut body.buf.as_slice_mut()[..buflen]) {
                        Ok(count) => {
                            log::debug!("rxrcv of {}", count);
                            body.valid = xous::MemorySize::new(count);
                            body.offset = xous::MemoryAddress::new(1);
                        }
                        Err(e) => {
                            log::debug!("unable to receive: {:?}", e);
                            body.offset = None;
                            body.valid = None;
                        }
                    }
                }

                // This block handles TCP Peek for libstd callers
                log::trace!("pump: tcp peek");
                for connection in tcp_peek_waiting.iter_mut() {
                    let socket;
                    let WaitingSocket {
                        mut env,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = iface.get_socket::<TcpSocket>(s.handle);
                                log::debug!("peek_state: {:?} {:?}", socket.state(), socket.local_endpoint());
                                if !socket.can_recv() {
                                    if let Some(trigger) = s.expiry {
                                        log::debug!("rx peek {:?}", trigger.get());
                                        if trigger.get() < now {
                                            // timer expired
                                        } else {
                                            continue;
                                        }
                                    } else if socket.state() == smoltcp::socket::TcpState::CloseWait
                                    // this state added to handle the auto-close edge case on a remote hang-up
                                    || socket.state() == smoltcp::socket::TcpState::Closed {
                                        // stop waiting if we're in CloseWait, as we don't plan to transmit
                                    } else {
                                        continue;
                                    }
                                }
                            }
                        }
                        connection.take().unwrap()
                    };

                    // If it can't receive, then the only explanation was that it timed out
                    if !socket.can_recv() {
                        if socket.state() == smoltcp::socket::TcpState::CloseWait
                        // this state added to handle the auto-close edge case on a remote hang-up
                        || socket.state() == smoltcp::socket::TcpState::Closed {
                            log::debug!("peekrcv connection closed");
                            let body = env.body.memory_message_mut().unwrap();
                            log::debug!("peekrcv of {}", 0);
                            body.valid = xous::MemorySize::new(0);
                            body.offset = xous::MemoryAddress::new(1);
                            continue;
                        } else {
                            log::debug!("peekrcv timed out");
                            respond_with_error(env, NetError::TimedOut);
                            continue;
                        }
                    }

                    let body = env.body.memory_message_mut().unwrap();
                    let buflen = if let Some(valid) = body.valid {
                        valid.get()
                    } else {
                        0
                    };
                    match socket.peek_slice(&mut body.buf.as_slice_mut()[..buflen]) {
                        Ok(count) => {
                            log::debug!("peekrcv of {}", count);
                            body.valid = xous::MemorySize::new(count);
                            body.offset = xous::MemoryAddress::new(1);
                        }
                        Err(e) => {
                            log::debug!("unable to peek: {:?}", e);
                            body.offset = None;
                            body.valid = None;
                        }
                    }
                }

                // This block handles TCP Tx for libstd callers
                log::trace!("pump: tcp tx");
                for connection in tcp_tx_waiting.iter_mut() {
                    let socket;
                    let WaitingSocket {
                        mut env,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = iface.get_socket::<TcpSocket>(s.handle);
                                log::debug!("tx_state: {:?} {:?}", socket.state(), socket.local_endpoint());
                                if socket.state() == smoltcp::socket::TcpState::Closed {
                                    // stop waiting if the stocket just closed on us outright
                                } else if !socket.can_send() {
                                    if let Some(trigger) = s.expiry {
                                        if trigger.get() < now {
                                            // timer expired
                                        } else {
                                            continue;
                                        }
                                    } else {
                                        continue;
                                    }
                                }
                            }
                        }
                        connection.take().unwrap()
                    };

                    if !socket.can_send() || socket.state() == smoltcp::socket::TcpState::Closed {
                        respond_with_error(env, NetError::TimedOut);
                        continue;
                    }

                    let body = env.body.memory_message_mut().unwrap();
                    // Perform the transfer
                    let sent_octets = {
                        let data = body.buf.as_slice::<u8>();
                        let length = body
                            .valid
                            .map(|v| {
                                if v.get() > data.len() {
                                    data.len()
                                } else {
                                    v.get()
                                }
                            })
                            .unwrap_or_else(|| data.len());

                        match socket.send_slice(&data[..length]) {
                            Ok(octets) => octets,
                            Err(_) => {
                                respond_with_error(env, NetError::LibraryError);
                                *connection = None;
                                continue;
                            }
                        }
                    };

                    log::trace!("sent {}", sent_octets);
                    let response_data = body.buf.as_slice_mut::<u32>();
                    response_data[0] = 0;
                    response_data[1] = sent_octets as u32;
                }

                // this handles TCP std listeners
                log::trace!("pump: tcp listen");
                for connection in tcp_accept_waiting.iter_mut() {
                    let ep: IpEndpoint;
                    let AcceptingSocket {
                        mut env,
                        handle: _,
                        fd,
                    } = match connection {
                        &mut None => continue,
                        Some(s) => {
                            let socket = iface.get_socket::<TcpSocket>(s.handle);
                            if socket.is_active() {
                                tcp_server_remote_close_poll.push(s.handle);
                                ep = socket.remote_endpoint();
                                connection.take().unwrap()
                            } else {
                                continue;
                            }
                        }
                    };
                    let body = env.body.memory_message_mut().unwrap();
                    let buf = body.buf.as_slice_mut::<u8>();

                    tcp_accept_success(buf, fd as u16, ep);
                }

                // this block handles StdUdp
                log::trace!("pump: udp");
                for connection in udp_rx_waiting.iter_mut() {
                    let socket;
                    let UdpStdState {
                        mut msg,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = iface.get_socket::<UdpSocket>(s.handle);
                                if !socket.can_recv() {
                                    if let Some(trigger) = s.expiry {
                                        if trigger < now {
                                            // timer expired
                                        } else {
                                            continue;
                                        }
                                    } else {
                                        continue;
                                    }
                                } // we don't process the Rx here because we need to `take()` the message first, so that its lifetime ends
                            }
                        }
                        // remove the connection from the list, allowing subsequent code to operate on it and then .drop()
                        connection.take().unwrap()
                    };

                    // If it can't receive, then the only explanation was that it timed out
                    if !socket.can_recv() {
                        std_failure(msg, NetError::TimedOut);
                        continue;
                    }

                    // Extract the receive data here; the `msg` will go out of scope at this point.
                    let do_peek = msg.body.memory_message().unwrap().offset.is_some();
                    if do_peek {
                        match socket.peek() {
                            Ok((data, endpoint)) => {
                                udp_rx_success(
                                    // unwrap is safe here because the message was type-checked prior to insertion into the waiting queue
                                    msg.body.memory_message_mut().unwrap().buf.as_slice_mut(),
                                    data,
                                    *endpoint // have to duplicate the code between peek and recv because of this type difference
                                );
                            }
                            Err(e) => {
                                log::error!("unable to receive: {:?}", e);
                                std_failure(msg, NetError::LibraryError);
                            }
                        }
                    } else {
                        match socket.recv() {
                            Ok((data, endpoint)) => {
                                log::debug!("netpump udp rx");
                                udp_rx_success(
                                    // unwrap is safe here because the message was type-checked prior to insertion into the waiting queue
                                    msg.body.memory_message_mut().unwrap().buf.as_slice_mut(),
                                    data,
                                    endpoint
                                );
                            }
                            Err(e) => {
                                log::error!("unable to receive: {:?}", e);
                                std_failure(msg, NetError::LibraryError);
                            }
                        }
                    }
                }

                log::trace!("pump: tcp close");
                tcp_tx_closing.retain(|(handle, sender)| {
                    if std_tcp_can_close(&tcp_tx_waiting, *handle) {
                        let socket = iface.get_socket::<TcpSocket>(*handle);
                        log::trace!("may_send: {}, send_queue: {}", socket.may_send(), socket.send_queue());
                        // different condition than the previous wait -- here we opportunistically close
                        // when either condition is met.
                        if !socket.may_send() || socket.send_queue() == 0 {
                            socket.close();
                            tcp_tx_wait_fin.push((*handle, *sender, 0));
                            //log::info!("EARLY CLOSE");
                            //xous::return_scalar(*sender, 0).ok(); // ack early so we don't block other processes waiting to close
                            false
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                });

                tcp_tx_wait_fin.retain_mut(|(handle, sender, count)| {
                    let socket = iface.get_socket::<TcpSocket>(*handle);
                    // count is a heuristic to stop TcpClose from blocking too long
                    // most implementations are fully non-blocking, we need to block on Xous
                    // to allow smoltcp to process correctly. However, the socket will stick
                    // around forever if the final ack doesn't arrive, and this gums up the works.
                    // FORCE_CLOSE_COUNT is an arbitrary threshold where we just decide to stop waiting for the other
                    // side to send the final ack: it's long enough that it almost never times out
                    // incorrectly, but short enough that we're not keeping around baggage forever.
                    const FORCE_CLOSE_COUNT: u32 = 16;
                    if !socket.is_open() || *count > FORCE_CLOSE_COUNT {
                        if *count > FORCE_CLOSE_COUNT {
                            log::warn!("forced close on {:?}", socket.local_endpoint());
                        }
                        log::debug!("socket closed {:?}", socket.local_endpoint());
                        iface.remove_socket(*handle);
                        tcp_server_remote_close_poll.retain(|x| {
                            *x != *handle
                        });
                        // log::info!("would return_scalar now");
                        xous::return_scalar(*sender, 0).ok();
                        false
                    } else {
                        *count += 1;
                        log::debug!("socket waiting to close({}): {:?} {:?}->{:?}", count, socket.state(), socket.local_endpoint(), socket.remote_endpoint());
                        true
                    }
                });

                tcp_server_remote_close_poll.retain(|handle| {
                    let socket = iface.get_socket::<TcpSocket>(*handle);
                    log::debug!("remote close poll: state {:?} local {:?}", socket.state(), socket.local_endpoint());
                    if socket.state() == smoltcp::socket::TcpState::CloseWait {
                        // initiate the closing ack, but allow the explicit drop to remove the socket once the handle is finished
                        // this handles the special case that a stream was accepted, but then the client hangs up
                        // and the server isn't actively polling the loop. By pushing the socket to the "close"
                        // state, we allow the three-way close handshake to actually complete instead of hanging
                        // in a FIN-WAIT-2 state.
                        socket.close();
                    }
                    if !socket.is_open() {
                        false
                    } else {
                        true
                    }
                });

                // this block contains the ICMP Rx handler. Tx is initiated by an incoming message to the Net crate.
                log::trace!("pump: icmp");
                {
                    let socket = iface.get_socket::<IcmpSocket>(icmp_handle);
                    if !socket.is_open() {
                        log::error!("ICMP socket isn't open, something went wrong...");
                    }

                    if socket.can_recv() {
                        let (payload, _) = socket
                            .recv()
                            .expect("couldn't receive on socket despite asserting availability");
                        log::trace!("icmp payload: {:x?}", payload);

                        for (connection, waiting_queue) in ping_destinations.iter_mut() {
                            let remote_addr = connection.remote;
                            match remote_addr {
                                IpAddress::Ipv4(_) => {
                                    let icmp_packet = Icmpv4Packet::new_checked(&payload).unwrap();
                                    let icmp_repr =
                                        Icmpv4Repr::parse(&icmp_packet, &device_caps.checksum)
                                            .unwrap();
                                    if let Icmpv4Repr::EchoReply { seq_no, data, .. } = icmp_repr {
                                        log::trace!(
                                            "got icmp seq no {} / data: {:x?}",
                                            seq_no,
                                            data
                                        );
                                        if let Some(_) = waiting_queue.get(&seq_no) {
                                            let packet_timestamp_ms = NetworkEndian::read_i64(data);
                                            waiting_queue.remove(&seq_no);
                                            // use try_send_message because we don't want to block if the recipient's queue is full;
                                            // instead, the message is just dropped
                                            match xous::try_send_message(
                                                connection.cid,
                                                Message::new_scalar(
                                                    connection.retop,
                                                    NetPingCallback::NoErr.to_usize().unwrap(),
                                                    u32::from_be_bytes(
                                                        remote_addr.as_bytes().try_into().unwrap(),
                                                    )
                                                        as usize,
                                                    seq_no as usize,
                                                    (now as i64 - packet_timestamp_ms) as usize,
                                                ),
                                            ) {
                                                Ok(_) => {}
                                                Err(xous::Error::ServerQueueFull) => {
                                                    log::warn!("Got seq {} response, but upstream server queue is full; dropping.", &seq_no);
                                                }
                                                Err(e) => {
                                                    log::error!(
                                                        "Unhandled error: {:?}; ignoring",
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    } else if let Icmpv4Repr::DstUnreachable {
                                        reason,
                                        header,
                                        ..
                                    } = icmp_repr
                                    {
                                        log::warn!(
                                            "Got dst unreachable {:?}: {:?}",
                                            header.dst_addr,
                                            reason
                                        );
                                        let reason_code: u8 = From::from(reason);
                                        match xous::try_send_message(
                                            connection.cid,
                                            Message::new_scalar(
                                                connection.retop,
                                                NetPingCallback::Unreachable.to_usize().unwrap()
                                                    | (reason_code as usize) << 24,
                                                u32::from_be_bytes(
                                                    remote_addr.as_bytes().try_into().unwrap(),
                                                )
                                                    as usize,
                                                0,
                                                0,
                                            ),
                                        ) {
                                            Ok(_) => {}
                                            Err(xous::Error::ServerQueueFull) => {
                                                log::warn!("Got dst {:?} unreachable, but upstream server queue is full; dropping.", remote_addr);
                                            }
                                            Err(e) => {
                                                log::error!("Unhandled error: {:?}; ignoring", e);
                                            }
                                        }
                                    } else {
                                        log::error!("got unhandled ICMP type, ignoring!");
                                    }
                                }

                                IpAddress::Ipv6(_) => {
                                    // NOTE: actually not sure what src_ipv6 should be. This is just from an example.
                                    let src_ipv6 = IpAddress::v6(0xfdaa, 0, 0, 0, 0, 0, 0, 1);
                                    let icmp_packet = Icmpv6Packet::new_checked(&payload).unwrap();
                                    let icmp_repr = Icmpv6Repr::parse(
                                        &remote_addr,
                                        &src_ipv6,
                                        &icmp_packet,
                                        &device_caps.checksum,
                                    )
                                    .unwrap();
                                    let ra = remote_addr.as_bytes();
                                    if let Icmpv6Repr::EchoReply { seq_no, data, .. } = icmp_repr {
                                        if let Some(_) = waiting_queue.get(&seq_no) {
                                            let packet_timestamp_ms = NetworkEndian::read_i64(data);
                                            waiting_queue.remove(&seq_no);
                                            match xous::try_send_message(
                                                connection.cid,
                                                Message::new_scalar(
                                                    connection.retop,
                                                    NetPingCallback::NoErr.to_usize().unwrap(),
                                                    u32::from_be_bytes(ra[..4].try_into().unwrap())
                                                        as usize,
                                                    u32::from_be_bytes(ra[12..].try_into().unwrap())
                                                        as usize,
                                                    (now as i64 - packet_timestamp_ms) as usize,
                                                ),
                                            ) {
                                                Ok(_) => {}
                                                Err(xous::Error::ServerQueueFull) => {
                                                    log::warn!("Got seq {} response, but upstream server queue is full; dropping.", &seq_no);
                                                }
                                                Err(e) => {
                                                    log::error!(
                                                        "Unhandled error: {:?}; ignoring",
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    } else if let Icmpv6Repr::DstUnreachable {
                                        reason,
                                        header,
                                        ..
                                    } = icmp_repr
                                    {
                                        let reason_code: u8 = From::from(reason);
                                        log::warn!(
                                            "Got dst unreachable {:?}: {:?}",
                                            header.dst_addr,
                                            reason
                                        );
                                        match xous::try_send_message(
                                            connection.cid,
                                            Message::new_scalar(
                                                connection.retop,
                                                NetPingCallback::Unreachable.to_usize().unwrap()
                                                    | (reason_code as usize) << 24,
                                                u32::from_be_bytes(ra[..4].try_into().unwrap())
                                                    as usize,
                                                u32::from_be_bytes(ra[8..12].try_into().unwrap())
                                                    as usize,
                                                u32::from_be_bytes(ra[12..].try_into().unwrap())
                                                    as usize,
                                            ),
                                        ) {
                                            Ok(_) => {}
                                            Err(xous::Error::ServerQueueFull) => {
                                                log::warn!("Got dst {:?} unreachable, but upstream server queue is full; dropping.", remote_addr);
                                            }
                                            Err(e) => {
                                                log::error!("Unhandled error: {:?}; ignoring", e);
                                            }
                                        }
                                    } else {
                                        log::error!("got unhandled ICMP type, ignoring!");
                                    }
                                }
                                _ => unimplemented!(),
                            }
                        }
                    }
                }
                // this block handles ICMP retirement; it runs everytime we pump the block
                log::trace!("pump: icmp retirement");
                {
                    // notify the callback to drop its connection, because the queue is now empty
                    // do this before we clear the queue, because we want the Drop message to come on the iteration
                    // *after* the queue is empty.
                    ping_destinations.retain(|conn, v|
                        if v.len() == 0 {
                            log::debug!("Dropping ping record for {:?}", conn.remote);
                            let ra = conn.remote.as_bytes();
                            match xous::send_message(conn.cid,
                                Message::new_scalar( // we should wait if the queue is full, as the "Drop" message is important
                                    conn.retop,
                                    NetPingCallback::Drop.to_usize().unwrap(),
                                    u32::from_be_bytes(ra[..4].try_into().unwrap()) as usize,
                                    if ra.len() == 16 {u32::from_be_bytes(ra[12..16].try_into().unwrap()) as usize} else {0},
                                    0,
                                )
                            ) {
                                Ok(_) => {},
                                Err(xous::Error::ServerNotFound) => {
                                    log::debug!("Server already dropped before we could send it a drop message. Ignoring.");
                                }
                                Err(e) => {
                                    panic!("couldn't send Drop on empty queue from Ping server: {:?}", e);
                                }
                            }
                            match unsafe{xous::disconnect(conn.cid)} {
                                Ok(_) => {},
                                Err(xous::Error::ServerNotFound) => {
                                    log::debug!("Disconnected from a server that has already disappeared. Moving on.");
                                }
                                Err(e) => {
                                    panic!("Unhandled error disconnecting from ping server: {:?}", e);
                                }
                            }
                            false
                        } else {
                            true
                        }
                    );

                    // now: sequence through the waiting_queue and remove entries that have hit our timeout
                    for (conn, waiting_queue) in ping_destinations.iter_mut() {
                        let ra = conn.remote.as_bytes();
                        waiting_queue.retain(|&seq, &mut start_time|
                            if now - start_time > ping_timeout_ms as u64 {
                                log::debug!("timeout - removing {:?}, {}", conn.remote, seq);
                                match xous::try_send_message(conn.cid,
                                    Message::new_scalar( // we should wait if the queue is full, as the "Drop" message is important
                                        conn.retop,
                                        NetPingCallback::Timeout.to_usize().unwrap(),
                                        u32::from_be_bytes(ra[..4].try_into().unwrap()) as usize,
                                        seq as usize,
                                        (now - start_time) as usize,
                                    )
                                ) {
                                    Ok(_) => {},
                                    Err(xous::Error::ServerQueueFull) => {
                                        log::warn!("Got dst {:?} timeout, but upstream server queue is full; dropping.", conn.remote);
                                    },
                                    Err(xous::Error::ServerNotFound) => {
                                        log::debug!("Callback server disappeared before we could inform it of timeout on {:?}, seq {}", conn.remote, seq);
                                    },
                                    Err(e) => {
                                        log::error!("Unhandled error: {:?}; ignoring", e);
                                    }
                                }
                                false
                            } else {
                                true
                            }
                        );
                    }
                }

                // establish our next check-up interval
                log::trace!("pump: checkup");
                let timestamp = Instant::from_millis(now as i64);
                if let Some(delay) = iface.poll_delay(timestamp) {
                    const RANGE_MS: u64 = 100; // this will define the responsivity to "future" events that maybe 100's of ms away
                    let mut wakeup_needed = false;
                    // round all short delays to within the next quantum
                    if (delay.total_millis() as u32) < xous::BASE_QUANTA_MS {
                        if wakeup_time.range(
                            ..
                            Wakeup { tx_index: 0, time: now + xous::BASE_QUANTA_MS as u64 }
                        ).count() == 0 {
                            wakeup_needed = true;
                        }
                    }
                    let wakeup = now + delay.total_millis();
                    // now check for longer delays over a longer window
                    if wakeup_time.range(
                        // "round up" to now if the window goes earlier than now
                        Wakeup { tx_index: 0, time: (wakeup - RANGE_MS).max(now) }
                        ..
                        Wakeup { tx_index: 0, time: wakeup + RANGE_MS }
                    ).count() == 0 {
                        wakeup_needed = true;
                    }
                    if wakeup_needed {
                        // find a free worker. this is an O(n) search right now, but generally (n) is small
                        let mut found = false;
                        for (index, worker) in wakeup_workers.iter_mut().enumerate() {
                            if !worker.is_busy {
                                worker.is_busy = true;
                                worker.time_replica = wakeup;
                                match worker.tx.send(wakeup) {
                                    Err(e) => log::error!("{}: Couldn't send wakeup time to worker {}: {}", e, index, wakeup),
                                    _ => {}
                                };
                                wakeup_time.insert(Wakeup {
                                    tx_index: index,
                                    time: wakeup,
                                });
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            log::warn!("All wakeup threads occupied, punting on wakeup at {}ms in the future", delay.total_millis());
                            let mut dbg = String::from("Now pending: ");
                            for (index, worker) in wakeup_workers.iter().enumerate() {
                                if worker.is_busy {
                                    dbg.push_str(&format!("{}[{}], ", index, worker.time_replica as i64 - now as i64));
                                }
                            }
                            log::info!("{}", dbg);
                        }
                    }
                }
            }),
            Some(Opcode::GetIpv4Config) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let ser = if let Some(config) = net_config {
                    Some(config.encode_u16())
                } else {
                    None
                };
                buffer.replace(ser).expect("couldn't return config");
            }
            Some(Opcode::SubscribeWifiStats) => {
                msg.forward(
                    cm_cid,
                    connection_manager::ConnectionManagerOpcode::SubscribeWifiStats as _)
                .expect("couldn't forward subscription request");
            }
            Some(Opcode::UnsubWifiStats) => {
                msg.forward(
                    cm_cid,
                    connection_manager::ConnectionManagerOpcode::UnsubWifiStats as _)
                .expect("couldn't forward unsub request");
            },
            Some(Opcode::FetchSsidList) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let ret_storage = SsidList::default();
                let mut buf =
                    Buffer::into_buf(ret_storage).expect("couldn't convert to memory message");
                buf.lend_mut(
                    cm_cid,
                    connection_manager::ConnectionManagerOpcode::FetchSsidList
                        .to_u32()
                        .unwrap(),
                )
                .expect("couldn't forward ssid list request");
                let ret_list = buf
                    .to_original::<SsidList, _>()
                    .expect("couldn't restore original");
                buffer.replace(ret_list).expect("couldn't return config");
            }
            Some(Opcode::ConnMgrStartStop) => msg_scalar_unpack!(msg, code, _, _, _, {
                if code == 0 {
                    // 0 is stop, 1 is start
                    send_message(
                        cm_cid,
                        Message::new_scalar(
                            connection_manager::ConnectionManagerOpcode::Stop
                                .to_usize()
                                .unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send scan stop message");
                } else if code == 1 {
                    send_message(
                        cm_cid,
                        Message::new_scalar(
                            connection_manager::ConnectionManagerOpcode::Run
                                .to_usize()
                                .unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send scan run message");
                } else if code == 2 {
                    send_message(
                        cm_cid,
                        Message::new_scalar(
                            connection_manager::ConnectionManagerOpcode::DisconnectAndStop
                                .to_usize()
                                .unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send wifi disconnect and stop message");
                } else if code == 3 {
                    send_message(
                        cm_cid,
                        Message::new_scalar(
                            connection_manager::ConnectionManagerOpcode::WifiOnAndRun
                                .to_usize()
                                .unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send wifi on and run message");
                } else if code == 4 {
                    send_message(
                        cm_cid,
                        Message::new_scalar(
                            connection_manager::ConnectionManagerOpcode::WifiOn
                                .to_usize()
                                .unwrap(),
                            0,
                            0,
                            0,
                            0,
                        ),
                    )
                    .expect("couldn't send wifi on message");
                } else {
                    log::error!("Got incorrect start/stop code: {}", code);
                }
            }),
            Some(Opcode::Reset) => {
                // reset the DHCP address
                IPV4_ADDRESS.store(0, Ordering::SeqCst);
                // ack any pending ints
                com_int_list.clear();
                com.ints_get_active(&mut com_int_list).ok();
                com.ints_ack(&com_int_list);
                // re-enable the interrupts as we intended
                set_com_ints(&mut com_int_list);
                com.ints_enable(&com_int_list);
                com_int_list.clear();

                // note: ARP cache isn't reset
                iface.routes_mut().remove_default_ipv4_route();
                dns_allclear_hook.notify();

                send_message(
                    cm_cid,
                    Message::new_scalar( // this has to be non-blocking to avoid deadlock: reset can be called from inside connection_manager
                        connection_manager::ConnectionManagerOpcode::EcReset
                            .to_usize()
                            .unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .expect("couldn't send EcReset message");
                xous::return_scalar(msg.sender, 1).unwrap();
            }
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                com_int_list.clear();
                com.ints_enable(&com_int_list); // disable all the interrupts

                susres
                    .suspend_until_resume(token)
                    .expect("couldn't execute suspend/resume");
                // re-enable the interrupts
                com_int_list.clear();
                com_int_list.push(ComIntSources::WlanIpConfigUpdate);
                com_int_list.push(ComIntSources::WlanRxReady);
                com_int_list.push(ComIntSources::BatteryCritical);
                com_int_list.push(ComIntSources::Connect);
                com_int_list.push(ComIntSources::Disconnect);
                com_int_list.push(ComIntSources::WlanSsidScanUpdate);
                com_int_list.push(ComIntSources::WlanSsidScanFinished);
                com_int_list.push(ComIntSources::WfxErr);
                com.ints_enable(&com_int_list);
            }),
            Some(Opcode::Quit) => {
                log::warn!("quit received");
                break;
            }
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xous::send_message(
        cm_cid,
        Message::new_blocking_scalar(
            connection_manager::ConnectionManagerOpcode::Quit
                .to_usize()
                .unwrap(),
            0,
            0,
            0,
            0,
        ),
    )
    .expect("couldn't quit connection manager server");
    unsafe { xous::disconnect(cm_cid).ok() };
    xns.unregister_server(net_sid).unwrap();
    xous::destroy_server(net_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
