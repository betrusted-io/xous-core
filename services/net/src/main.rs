#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use com::api::{ComIntSources, Ipv4Conf, NET_MTU};
use num_traits::*;
use ticktimer_server::Ticktimer;

mod connection_manager;
mod device;

use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, send_message, Message, CID, SID};
use xous_ipc::Buffer;

use byteorder::{ByteOrder, NetworkEndian};
use smoltcp::iface::{Interface, InterfaceBuilder, NeighborCache, Routes};
use smoltcp::phy::{Device, Medium};
use smoltcp::socket::{IcmpEndpoint, IcmpPacketMetadata, IcmpSocket, IcmpSocketBuffer, SocketSet};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv4Cidr};
use smoltcp::wire::{Icmpv4Packet, Icmpv4Repr, Icmpv6Packet, Icmpv6Repr};

use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU32, Ordering};
use smoltcp::socket::{
    SocketHandle, TcpSocket, TcpSocketBuffer, UdpPacketMetadata, UdpSocket, UdpSocketBuffer,
};
use smoltcp::time::{Duration, Instant};
use std::sync::Arc;
use std::thread;

const PING_DEFAULT_TIMEOUT_MS: u32 = 10_000;

fn set_ipv4_addr<DeviceT>(iface: &mut Interface<'_, DeviceT>, cidr: Ipv4Cidr)
where
    DeviceT: for<'d> Device<'d>,
{
    iface.update_ip_addrs(|addrs| {
        let dest = addrs
            .iter_mut()
            .next()
            .expect("trouble updating ipv4 addresses in routing table");
        *dest = IpCidr::Ipv4(cidr);
    });
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
enum WaitOp {
    WaitMs,
    PollAt,
    Quit,
}

/// UdpState will return a full custom datastructure, and is designed to work with
/// a one-time use dedicated server created as part of the Net library code.
pub struct UdpState {
    handle: SocketHandle,
    cid: CID,
    sid: SID,
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

#[derive(Hash, PartialEq, Eq)]
pub struct TcpConnection {
    remote: IpAddress,
    remote_port: u16,
    local_port: u16,
}
#[derive(Copy, Clone, Debug)]
pub struct TcpState {
    handle: SocketHandle,
    cid: CID,
    shutdown_rx: bool,
}

struct WaitingSocket {
    env: xous::MessageEnvelope,
    handle: SocketHandle,
    expiry: Option<NonZeroU64>,
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
}

fn parse_address(data: &[u8]) -> Option<smoltcp::wire::IpAddress> {
    let mut i = data.iter();
    match i.next() {
        Some(&4) => Some(smoltcp::wire::IpAddress::v4(
            *i.next()?,
            *i.next()?,
            *i.next()?,
            *i.next()?,
        )),
        Some(&6) => {
            let mut new_addr = [0u8; 16];
            for octet in new_addr.iter_mut() {
                *octet = *i.next()?;
            }
            let v6: std::net::Ipv6Addr = new_addr.into();
            Some(v6.into())
        }
        _ => None,
    }
}

fn write_address(address: IpAddress, data: &mut [u8]) -> Option<usize> {
    let mut i = data.iter_mut();
    match address {
        IpAddress::Ipv4(a) => {
            *i.next()? = 4;
            for (dest, src) in i.zip(a.as_bytes().iter()) {
                *dest = *src;
            }
            Some(5)
        }
        IpAddress::Ipv6(a) => {
            *i.next()? = 6;
            for (dest, src) in i.zip(a.as_bytes().iter()) {
                *dest = *src;
            }
            Some(16)
        }
        _ => {
            *i.next()? = 0;
            Some(1)
        }
    }
}

fn respond_with_error(mut env: xous::MessageEnvelope, code: NetError) -> Option<()> {
    // If it's not a memory message, don't fill in the return information.
    let body = match env.body.memory_message_mut() {
        None => {
            // But do respond to the scalar message, if it's a BlockingScalar
            if env.body.scalar_message().is_some() && env.body.is_blocking() {
                xous::return_scalar(env.sender, code as usize).ok();
            }
            return None;
        }
        Some(b) => b,
    };

    body.valid = None;
    let s: &mut [u8] = body.buf.as_slice_mut();
    let mut i = s.iter_mut();

    // Duplicate error to ensure it's seen as an error regardless of byte order
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = 1;
    *i.next()? = code as u8;
    *i.next()? = 0;
    *i.next()? = 0;
    *i.next()? = 0;
    None
}

fn respond_with_connected(
    mut env: xous::MessageEnvelope,
    idx: u16,
    local_port: u16,
    remote_port: u16,
) {
    let body = env.body.memory_message_mut().unwrap();
    let bfr = body.buf.as_slice_mut::<u16>();

    log::trace!("successfully connected: {}", idx);
    bfr[0] = 0;
    bfr[1] = idx;
    bfr[2] = local_port;
    bfr[3] = remote_port;
}

/// Insert `Some(value)` into the first slot in the Vec that is `None`,
/// or append it to the end if there is no free slot
fn insert_or_append<T>(arr: &mut Vec<Option<T>>, val: T) -> usize {
    // Look for a free index, or add it onto the end.
    let mut idx = None;
    for (i, elem) in arr.iter_mut().enumerate() {
        if elem.is_none() {
            idx = Some(i);
            break;
        }
    }
    if let Some(idx) = idx {
        arr[idx] = Some(val);
        idx
    } else {
        let idx = arr.len();
        arr.push(Some(val));
        idx
    }
}

fn std_tcp_connect(
    mut msg: xous::MessageEnvelope,
    local_port: u16,
    sockets: &mut SocketSet,
    tcp_connect_waiting: &mut Vec<Option<(xous::MessageEnvelope, SocketHandle, u16, u16, u16)>>,
    our_sockets: &mut Vec<Option<SocketHandle>>,
) {
    // Ignore nonblocking and scalar messages
    let body = match msg.body.memory_message_mut() {
        Some(b) => b,
        None => {
            log::trace!("invalid message type");
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    let bytes = body.buf.as_slice::<u8>();
    let remote_port = u16::from_le_bytes([bytes[0], bytes[1]]);
    let timeout_ms = NonZeroU64::new(u64::from_le_bytes(bytes[2..10].try_into().unwrap()));
    let address = match parse_address(&bytes[10..]) {
        Some(addr) => addr,
        None => {
            log::trace!("couldn't parse address");
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // initiates a new connection to a remote server consisting of an (Address:Port) tuple.
    // multiple connections can exist to a server, and they are further differentiated by the return port
    let mut tcp_socket = TcpSocket::new(
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
        TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]),
    );

    // Attempt to connect, returning the error if there is one
    if let Err(e) = tcp_socket
        .connect((address, remote_port), local_port)
        .map_err(|e| match e {
            smoltcp::Error::Illegal => NetError::SocketInUse,
            smoltcp::Error::Unaddressable => NetError::Unaddressable,
            _ => NetError::LibraryError,
        })
    {
        log::trace!("couldn't connect: {:?}", e);
        respond_with_error(msg, e);
        return;
    }

    tcp_socket.set_timeout(timeout_ms.map(|t| Duration::from_millis(t.get())));

    let handle = sockets.add(tcp_socket);

    // Add the socket onto the list of sockets waiting to connect, since the connection will
    // take time.
    let idx = insert_or_append(our_sockets, handle) as u16;
    insert_or_append(
        tcp_connect_waiting,
        (msg, handle, idx, local_port, remote_port),
    );
}

fn std_tcp_tx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    sockets: &mut SocketSet,
    tcp_tx_waiting: &mut Vec<Option<WaitingSocket>>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if !socket.can_send() {
        log::trace!("tx can't send, will retry");
        let expiry = body
            .offset
            .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });
        // Add the message to the TcpRxWaiting list, which will prevent it from getting
        // responded to right away.
        insert_or_append(
            tcp_tx_waiting,
            WaitingSocket {
                env: msg,
                handle: *handle,
                expiry,
            },
        );
        return;
    }

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
                respond_with_error(msg, NetError::LibraryError);
                return;
            }
        }
    };

    log::trace!("sent {}", sent_octets);
    let response_data = body.buf.as_slice_mut::<u32>();
    body.valid = xous::MemorySize::new(sent_octets);
    response_data[0] = 0;
    response_data[1] = sent_octets as u32;
}

fn std_tcp_rx(
    mut msg: xous::MessageEnvelope,
    timer: &Ticktimer,
    sockets: &mut SocketSet,
    tcp_rx_waiting: &mut Vec<Option<WaitingSocket>>,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // Default to having no valid data upon return, indicating an error
    body.valid = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::trace!("receiving data right away");
        match socket.recv_slice(body.buf.as_slice_mut()) {
            Ok(bytes) => {
                body.valid = xous::MemorySize::new(bytes);
                log::trace!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
        return;
    }

    log::trace!("socket was not able to receive, adding it to list of waiting messages");

    // Add the message to the TcpRxWaiting list, which will prevent it from getting
    // responded to right away.
    let expiry = body
        .offset
        .map(|x| unsafe { NonZeroU64::new_unchecked(x.get() as u64 + timer.elapsed_ms()) });
    insert_or_append(
        tcp_rx_waiting,
        WaitingSocket {
            env: msg,
            handle: *handle,
            expiry,
        },
    );
}

fn std_tcp_peek(
    mut msg: xous::MessageEnvelope,
    sockets: &mut SocketSet,
    our_sockets: &Vec<Option<SocketHandle>>,
) {
    let connection_handle_index = (msg.body.id() >> 16) & 0xffff;
    let body = match msg.body.memory_message_mut() {
        Some(body) => body,
        None => {
            respond_with_error(msg, NetError::LibraryError);
            return;
        }
    };

    // Default to having no valid data upon return, indicating an error
    body.valid = None;

    let handle = match our_sockets.get(connection_handle_index) {
        Some(Some(val)) => val,
        _ => {
            respond_with_error(msg, NetError::Invalid);
            return;
        }
    };

    let mut socket = sockets.get::<TcpSocket>(*handle);
    if socket.can_recv() {
        log::trace!("receiving data right away");
        match socket.peek_slice(body.buf.as_slice_mut()) {
            Ok(bytes) => {
                body.valid = xous::MemorySize::new(bytes);
                log::trace!("set body.valid = {:?}", body.valid);
            }
            Err(e) => {
                log::error!("unable to receive: {:?}", e);
                respond_with_error(msg, NetError::LibraryError);
            }
        }
    } else {
        // No data available, so indicate `None`
        body.valid = None;
        // Also indicate no error
        body.buf.as_slice_mut::<u32>()[0] = 0;
    }
}

#[xous::xous_main]
fn xmain() -> ! {
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
    const MAX_DELAY_THREADS: u32 = 10; // limit the number of concurrent delay threads. Typically we have 1-2 running at any time, but DoS conditions could lead to many more.
    let delay_threads = Arc::new(AtomicU32::new(0));
    let mut net_config: Option<Ipv4Conf> = None;

    // storage for all our sockets
    let mut sockets = SocketSet::new(vec![]);

    // Each process keeps track of its own sockets. These are kept in a Vec. When a handle
    // is destroyed, it is turned into a `None`.
    let mut process_sockets: HashMap<Option<xous::PID>, Vec<Option<SocketHandle>>> = HashMap::new();

    // When a client issues a Receive request, it will get placed here while the packet data
    // is being accumulated.
    let mut tcp_rx_waiting: Vec<Option<WaitingSocket>> = Vec::new();

    // When a client issues a Send request, it will get placed here while the packet data
    // is being accumulated.
    let mut tcp_tx_waiting: Vec<Option<WaitingSocket>> = Vec::new();

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
    let mut icmp_socket = IcmpSocket::new(icmp_rx_buffer, icmp_tx_buffer);
    let ident = 0x22b;
    icmp_socket
        .bind(IcmpEndpoint::Ident(ident))
        .expect("couldn't bind to icmp socket");
    let icmp_handle = sockets.add(icmp_socket);
    let mut seq: u16 = 0;
    // this record stores the origin time + IP address of the outgoing ping sequence number
    let mut ping_destinations = HashMap::<PingConnection, HashMap<u16, u64>>::new();
    let mut ping_timeout_ms = PING_DEFAULT_TIMEOUT_MS;

    // udp storage
    let mut udp_handles = HashMap::<u16, UdpState>::new();
    // UDP requires multiple copies. The way it works is that Tx can come from anyone;
    // for Rx, copies of a CID,SID tuple are kept for every clone is kept in a HashMap. This
    // allows for the Rx data to be cc:'d to each clone, and identified by SID upon drop
    let mut udp_clones = HashMap::<u16, HashMap<[u32; 4], CID>>::new(); // additional clones for UDP responders

    // tcp storage
    let mut tcp_handles = HashMap::<TcpConnection, TcpState>::new();
    let mut tcp_listeners = HashMap::<u16, Vec<TcpState>>::new();

    // other link storage
    let neighbor_cache = NeighborCache::new(BTreeMap::new());
    let ip_addrs = [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)];
    let routes = Routes::new(BTreeMap::new());

    let device = device::NetPhy::new(&xns);
    // needed by ICMP to determine if we should compute checksums
    let device_caps = device.capabilities();
    let medium = device.capabilities().medium;
    let mut builder = InterfaceBuilder::new(device)
        .ip_addrs(ip_addrs)
        .routes(routes);
    if medium == Medium::Ethernet {
        builder = builder
            .ethernet_addr(EthernetAddress::from_bytes(&[0; 6]))
            .neighbor_cache(neighbor_cache);
    }
    let mut iface = builder.finalize();

    // DNS hooks - the DNS server can ask the Net crate to tickle it when IP configs change using these hooks
    // Currently, we assume there is only one DNS server in Xous. I suppose you could
    // upgrade the code to handle multiple DNS servers, but...why???
    // ... nevermind, someone will invent a $reason because there was never a shiny
    // new feature that a coder didn't love and *had* to have *right now*.
    let mut dns_ipv4_hook = XousScalarEndpoint::new();
    let mut dns_ipv6_hook = XousScalarEndpoint::new();
    let mut dns_allclear_hook = XousScalarEndpoint::new();

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
        match FromPrimitive::from_usize(msg.body.id() & 0xffff) {
            Some(Opcode::Ping) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut pkt = buf.to_original::<NetPingPacket, _>().unwrap();
                let mut socket = sockets.get::<IcmpSocket>(icmp_handle);
                if socket.can_send() {
                    log::trace!("sending ping to {:?}", pkt.endpoint);
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
                                ident,
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
                                ident,
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
                let mut socket = sockets.get::<IcmpSocket>(icmp_handle);
                socket.set_hop_limit(Some(checked_ttl));
            }),
            Some(Opcode::PingGetTtl) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let socket = sockets.get::<IcmpSocket>(icmp_handle);
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
                // Pick a random locak port using the system's TRNG
                let local_port = (trng.get_u32().unwrap() % 16384 + 49152) as u16;
                let pid = msg.sender.pid();

                std_tcp_connect(
                    msg,
                    local_port,
                    &mut sockets,
                    &mut tcp_connect_waiting,
                    process_sockets.entry(pid).or_default(),
                );
            }

            Some(Opcode::StdTcpTx) => {
                let pid = msg.sender.pid();
                std_tcp_tx(
                    msg,
                    &timer,
                    &mut sockets,
                    &mut tcp_tx_waiting,
                    process_sockets.entry(pid).or_default(),
                );
            }

            Some(Opcode::StdTcpPeek) => {
                let pid = msg.sender.pid();
                std_tcp_peek(msg, &mut sockets, process_sockets.entry(pid).or_default());
            }

            Some(Opcode::StdTcpRx) => {
                let pid = msg.sender.pid();
                std_tcp_rx(
                    msg,
                    &timer,
                    &mut sockets,
                    &mut tcp_rx_waiting,
                    process_sockets.entry(pid).or_default(),
                );
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
                sockets.get::<TcpSocket>(handle).close();
                sockets.remove(handle);
                if let Some(response) = msg.body.memory_message_mut() {
                    response.buf.as_slice_mut::<u8>()[0] = 0;
                } else if !msg.body.is_blocking() && msg.body.is_blocking() {
                    xous::return_scalar(msg.sender, 0).ok();
                }
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
                    let socket = sockets.get::<TcpSocket>(*connection);
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
                    let socket = sockets.get::<TcpSocket>(*connection);
                    xous::return_scalar(
                        msg.sender,
                        socket.hop_limit().unwrap_or_default() as usize,
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
                    let mut socket = sockets.get::<TcpSocket>(*connection);
                    let args = msg.body.scalar_message().unwrap();
                    let hop_limit = if args.arg1 == 0 {
                        None
                    } else {
                        Some(args.arg1 as u8)
                    };
                    socket.set_hop_limit(hop_limit);
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
                    let socket = sockets.get::<TcpSocket>(*connection);
                    xous::return_scalar(
                        msg.sender,
                        if socket.nagle_enabled().is_some() {
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
                    let mut socket = sockets.get::<TcpSocket>(*connection);
                    let args = msg.body.scalar_message().unwrap();
                    socket.set_nagle_enabled(args.arg1 != 0);
                    xous::return_scalar(msg.sender, 0).ok();
                } else {
                    respond_with_error(msg, NetError::Invalid);
                };
            }

            Some(Opcode::TcpConnect) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcpspec = buf.to_original::<NetTcpManage, _>().unwrap(); // need to define this
                let address = IpAddress::from(tcpspec.ip_addr);
                let remote_port = tcpspec.remote_port;

                // initiates a new connection to a remote server consisting of an (Address:Port) tuple.
                // multiple connections can exist to a server, and they are further differentiated by the return port
                let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
                let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
                let mut tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
                if let Some(timeout) = tcpspec.timeout_ms {
                    tcp_socket.set_timeout(Some(Duration::from_millis(timeout)));
                }
                if let Some(keepalive) = tcpspec.keepalive_ms {
                    tcp_socket.set_keep_alive(Some(Duration::from_millis(keepalive)));
                }
                let local_port = (49152 + trng.get_u32().unwrap() % 16384) as u16;
                match tcp_socket.connect((address, remote_port), local_port) {
                    Ok(_) => {
                        let connection = TcpConnection {
                            remote: address,
                            remote_port,
                            local_port,
                        };
                        let handle = sockets.add(tcp_socket);
                        let sid = tcpspec.cb_sid;
                        let cid = xous::connect(SID::from_array(sid)).unwrap();
                        let tcp_cb_state = TcpState {
                            handle,
                            cid,
                            shutdown_rx: false,
                        };
                        tcp_handles.insert(connection, tcp_cb_state);
                        tcpspec.local_port = Some(local_port);
                        tcpspec.result = Some(NetMemResponse::Ok);
                    }
                    Err(e) => match e {
                        smoltcp::Error::Illegal => {
                            tcpspec.result = Some(NetMemResponse::SocketInUse);
                        }
                        smoltcp::Error::Unaddressable => {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                        _ => {
                            tcpspec.result = Some(NetMemResponse::LibraryError);
                        }
                    },
                }
                buf.replace(tcpspec).unwrap();
            }
            Some(Opcode::TcpManage) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcpspec = buf.to_original::<NetTcpManage, _>().unwrap();
                if let Some(local_port) = tcpspec.local_port {
                    let connection = TcpConnection {
                        remote: IpAddress::from(tcpspec.ip_addr),
                        remote_port: tcpspec.remote_port,
                        local_port,
                    };
                    if let Some(tcp_state) = tcp_handles.get_mut(&connection) {
                        if let Some(code) = tcpspec.mgmt_code {
                            match code {
                                TcpMgmtCode::SetRxShutdown => {
                                    tcp_state.shutdown_rx = true;
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::SetNoDelay(value) => {
                                    let mut socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    socket.set_nagle_enabled(value);
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::GetNoDelay(_) => {
                                    let socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    let value = socket.nagle_enabled().is_some();
                                    tcpspec.mgmt_code = Some(TcpMgmtCode::GetNoDelay(value));
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::SetTtl(mut ttl) => {
                                    let mut socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    if ttl > 255 {
                                        ttl = 255;
                                    }
                                    if ttl > 0 {
                                        socket.set_hop_limit(Some(ttl as u8));
                                    } else {
                                        socket.set_hop_limit(None);
                                    }
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::GetTtl(_) => {
                                    let socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    let value = socket.hop_limit().unwrap_or(64); // because that's what smoltcp does
                                    tcpspec.mgmt_code = Some(TcpMgmtCode::GetTtl(value as u32));
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::ErrorCheck(_) => {
                                    /* // I don't think there is actually any error check reporting available? `recv_error_check()` is a private function for smoltcp...
                                    let socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    tcpspec.mgmt_code = Some(TcpMgmtCode::ErrorCheck(
                                        match socket.recv_error_check() {
                                            Ok(_) => NetMemResponse::Ok,
                                            Err(smoltcp::Error::Finished) => NetMemResponse::Finished,
                                            Err(smoltcp::Error::Illegal) => NetMemResponse::Invalid,
                                            _ => Some(NetMemResponse::LibraryError),
                                        }
                                    ));*/
                                    tcpspec.mgmt_code =
                                        Some(TcpMgmtCode::ErrorCheck(NetMemResponse::Ok));
                                    tcpspec.result = Some(NetMemResponse::Ok);
                                }
                                TcpMgmtCode::Flush(_) => {
                                    let socket = sockets.get::<TcpSocket>(tcp_state.handle);
                                    if socket.send_queue() == 0 {
                                        tcpspec.mgmt_code = Some(TcpMgmtCode::Flush(true));
                                    } else {
                                        tcpspec.mgmt_code = Some(TcpMgmtCode::Flush(false));
                                    }
                                }
                                TcpMgmtCode::CloseListener => {
                                    // this is used for listener management, should never be sent to this routine
                                    tcpspec.result = Some(NetMemResponse::LibraryError);
                                }
                            }
                        } else {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                    } else {
                        tcpspec.result = Some(NetMemResponse::Invalid);
                    }
                } else {
                    tcpspec.result = Some(NetMemResponse::Invalid);
                }
                buf.replace(tcpspec).unwrap();
            }

            Some(Opcode::TcpTx) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcp_tx = buf.to_original::<NetTcpTransmit, _>().unwrap();
                let connection = TcpConnection {
                    remote: IpAddress::from(tcp_tx.remote_addr),
                    remote_port: tcp_tx.remote_port,
                    local_port: tcp_tx.local_port,
                };
                if let Some(tcp_state) = tcp_handles.get(&connection) {
                    let mut socket = sockets.get::<TcpSocket>(tcp_state.handle);
                    if socket.can_send() {
                        tcp_tx.result = match socket.send_slice(&tcp_tx.data[..tcp_tx.len as usize])
                        {
                            Ok(octets) => {
                                log::trace!("sent {}", octets);
                                tcp_tx.len = octets as u16;
                                Some(NetMemResponse::Sent(octets as u16))
                            }
                            Err(_) => Some(NetMemResponse::LibraryError),
                        }
                    } else {
                        log::trace!("tx can't send, please retry");
                        // inform the sender it should retry
                        tcp_tx.result = Some(NetMemResponse::SocketInUse);
                    }
                } else {
                    log::trace!("tx spec invalid");
                    tcp_tx.result = Some(NetMemResponse::Invalid);
                }
                buf.replace(tcp_tx).unwrap();
            }
            Some(Opcode::TcpClose) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcpspec = buf.to_original::<NetTcpManage, _>().unwrap(); // need to define this
                if let Some(local_port) = tcpspec.local_port {
                    let connection = TcpConnection {
                        remote: IpAddress::from(tcpspec.ip_addr),
                        remote_port: tcpspec.remote_port,
                        local_port,
                    };
                    if let Some(tcp_state) = tcp_handles.remove(&connection) {
                        sockets.get::<TcpSocket>(tcp_state.handle).close();
                        sockets.remove(tcp_state.handle);
                        tcpspec.result = Some(NetMemResponse::Ok);
                    } else {
                        tcpspec.result = Some(NetMemResponse::Invalid);
                    }
                } else {
                    tcpspec.result = Some(NetMemResponse::Invalid);
                }
                buf.replace(tcpspec).unwrap();
            }
            Some(Opcode::TcpListen) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcpspec = buf.to_original::<NetTcpListen, _>().unwrap();

                let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
                let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
                let mut tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);
                log::debug!("adding listener to local port {}", tcpspec.local_port);
                match tcp_socket.listen(tcpspec.local_port) {
                    Ok(_) => {
                        if let Some(list) = tcp_listeners.get(&tcpspec.local_port) {
                            if list.len() > 0 {
                                // empty check necessary because an empty vector is left if a socket went from listening->active, or was dropped
                                // guarantee that all TTLs are same even if we're inserting a new socket
                                let ttl = sockets.get::<TcpSocket>(list[0].handle).hop_limit();
                                tcp_socket.set_hop_limit(ttl);
                            }
                        }
                        let handle = sockets.add(tcp_socket);
                        let sid = tcpspec.cb_sid;
                        let cid = xous::connect(SID::from_array(sid)).unwrap();
                        log::trace!("Listener with cid {}, sid {:x?} registered", cid, sid);
                        let tcp_cb_state = TcpState {
                            handle,
                            cid,
                            shutdown_rx: false,
                        };
                        if let Some(list) = tcp_listeners.get_mut(&tcpspec.local_port) {
                            list.push(tcp_cb_state);
                            log::trace!(
                                "total listeners on port {}: {}",
                                tcpspec.local_port,
                                list.len()
                            );
                        } else {
                            tcp_listeners.insert(tcpspec.local_port, vec![tcp_cb_state]);
                            log::trace!("creating first listener on port {}", tcpspec.local_port);
                        }
                        tcpspec.result = Some(NetMemResponse::Ok);
                    }
                    Err(e) => match e {
                        smoltcp::Error::Illegal => {
                            tcpspec.result = Some(NetMemResponse::SocketInUse);
                        }
                        smoltcp::Error::Unaddressable => {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                        _ => {
                            tcpspec.result = Some(NetMemResponse::LibraryError);
                        }
                    },
                }
                buf.replace(tcpspec).unwrap();
            }
            Some(Opcode::TcpManageListener) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut tcpspec = buf.to_original::<NetTcpManage, _>().unwrap();
                match tcpspec.mgmt_code {
                    Some(TcpMgmtCode::CloseListener) => {
                        if let Some(listener) = tcp_listeners.get_mut(&tcpspec.local_port.unwrap())
                        {
                            if let Some(tcp_state) = listener.pop() {
                                sockets.get::<TcpSocket>(tcp_state.handle).close();
                                log::debug!(
                                    "closing one listener on port {:?}",
                                    tcpspec.local_port
                                );
                                sockets.remove(tcp_state.handle);
                                tcpspec.result = Some(NetMemResponse::Ok);
                                // this may leave an empty vector in the tcp_listeners structure, but I think that's OK
                            }
                        } else {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                    }
                    Some(TcpMgmtCode::SetTtl(mut ttl)) => {
                        if let Some(listener_vec) =
                            tcp_listeners.get_mut(&tcpspec.local_port.unwrap())
                        {
                            for listener in listener_vec.iter_mut() {
                                let mut socket = sockets.get::<TcpSocket>(listener.handle);
                                if ttl > 255 {
                                    ttl = 255;
                                }
                                if ttl > 0 {
                                    socket.set_hop_limit(Some(ttl as u8));
                                } else {
                                    socket.set_hop_limit(None);
                                }
                            }
                            tcpspec.result = Some(NetMemResponse::Ok);
                        } else {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                    }
                    Some(TcpMgmtCode::GetTtl(_)) => {
                        if let Some(listener) = tcp_listeners.get(&tcpspec.local_port.unwrap()) {
                            if listener.len() > 0 {
                                // all listeners "should" have an identical setting, so, just return the setting of the 0th one
                                let socket = sockets.get::<TcpSocket>(listener[0].handle);
                                let value = socket.hop_limit().unwrap_or(64); // because that's what smoltcp does
                                tcpspec.mgmt_code = Some(TcpMgmtCode::GetTtl(value as u32));
                                tcpspec.result = Some(NetMemResponse::Ok);
                            } else {
                                tcpspec.result = Some(NetMemResponse::Invalid);
                            }
                        } else {
                            tcpspec.result = Some(NetMemResponse::Invalid);
                        }
                    }
                    _ => tcpspec.result = Some(NetMemResponse::LibraryError),
                }
                buf.replace(tcpspec).unwrap();
            }
            Some(Opcode::UdpBind) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let udpspec = buf.to_original::<NetUdpBind, _>().unwrap();

                let buflen = if let Some(maxlen) = udpspec.max_payload {
                    maxlen as usize
                } else {
                    NET_MTU as usize
                };
                if udp_handles.contains_key(&udpspec.port) {
                    // if we're already connected, just register the extra listener in the clones array
                    let sid = udpspec.cb_sid;
                    let cid = xous::connect(SID::from_array(sid)).unwrap();
                    if let Some(clone_map) = udp_clones.get_mut(&udpspec.port) {
                        // if a clone already exists, put the additional clone into the map
                        match clone_map.insert(sid, cid) {
                            Some(_) => {
                                log::error!("Something went wrong in a UDP clone operation -- same SID registered twice");
                                buf.replace(NetMemResponse::SocketInUse).unwrap()
                            } // the same SID has double-registered, this is an error
                            None => buf.replace(NetMemResponse::Ok).unwrap(),
                        }
                    } else {
                        // otherwise, create the clone mapping entry
                        let mut newmap = HashMap::new();
                        newmap.insert(sid, cid);
                        udp_clones.insert(udpspec.port, newmap);
                    }
                    buf.replace(NetMemResponse::Ok).unwrap();
                } else {
                    let udp_rx_buffer =
                        UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; buflen]);
                    let udp_tx_buffer =
                        UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; buflen]);
                    let mut udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
                    match udp_socket.bind(udpspec.port) {
                        Ok(_) => {
                            let sid = SID::from_array(udpspec.cb_sid);
                            let udpstate = UdpState {
                                handle: sockets.add(udp_socket),
                                cid: xous::connect(sid).unwrap(),
                                sid,
                            };
                            udp_handles.insert(udpspec.port, udpstate);
                            buf.replace(NetMemResponse::Ok).unwrap();
                        }
                        Err(e) => {
                            log::error!("Udp couldn't bind to socket: {:?}", e);
                            buf.replace(NetMemResponse::Invalid).unwrap();
                        }
                    }
                }
            }
            Some(Opcode::UdpClose) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let udpspec = buf.to_original::<NetUdpBind, _>().unwrap();
                // need to find the SID that matches either in the clone array, or the primary binding.
                // first check the clone array, then fall back to the primary binding
                match udp_clones.get_mut(&udpspec.port) {
                    Some(clone_map) => match clone_map.remove(&udpspec.cb_sid) {
                        Some(cid) => {
                            cid_to_disconnect = Some(cid);
                            buf.replace(NetMemResponse::Ok).unwrap();
                            continue;
                        }
                        None => {}
                    },
                    None => {}
                }
                match udp_handles.remove(&udpspec.port) {
                    Some(udpstate) => {
                        if udpstate.sid == SID::from_array(udpspec.cb_sid) {
                            match udp_clones.get_mut(&udpspec.port) {
                                // if the clone map is nil, close the socket, we're done
                                None => {
                                    sockets.get::<UdpSocket>(udpstate.handle).close();
                                    sockets.remove(udpstate.handle);
                                    buf.replace(NetMemResponse::Ok).unwrap();
                                }
                                // if the clone map has entries, promote an arbitrary map entry to the primary handle
                                Some(clone_map) => {
                                    if clone_map.len() == 0 {
                                        // removing SIDs doesn't remove the map, so it's possible to have an empty mapping. Get rid of it, and we're done.
                                        udp_clones.remove(&udpspec.port);
                                        sockets.get::<UdpSocket>(udpstate.handle).close();
                                        sockets.remove(udpstate.handle);
                                        buf.replace(NetMemResponse::Ok).unwrap();
                                    } else {
                                        // take an arbitrary key, re-insert it into the handles map.
                                        let new_primary_sid = *clone_map.keys().next().unwrap(); // unwrap is appropriate because len already checked as not 0
                                        let udpstate = UdpState {
                                            handle: udpstate.handle,
                                            cid: *clone_map.get(&new_primary_sid).unwrap(),
                                            sid: SID::from_array(new_primary_sid),
                                        };
                                        udp_handles.insert(udpspec.port, udpstate);
                                        // now remove it from the clone map
                                        clone_map.remove(&new_primary_sid);
                                        // clean up the clone map if it's empty
                                        if clone_map.len() == 0 {
                                            udp_clones.remove(&udpspec.port);
                                        }
                                        buf.replace(NetMemResponse::Ok).unwrap();
                                    }
                                }
                            }
                        }
                    }
                    _ => buf.replace(NetMemResponse::Invalid).unwrap(),
                }
            }
            Some(Opcode::UdpTx) => {
                use std::convert::TryInto;
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let udp_tx = buf.to_original::<NetUdpTransmit, _>().unwrap();
                match udp_handles.get_mut(&udp_tx.local_port) {
                    Some(udpstate) => {
                        if let Some(dest_socket) = udp_tx.dest_socket {
                            let endpoint = IpEndpoint::new(
                                dest_socket.addr.try_into().unwrap(),
                                dest_socket.port,
                            );
                            let mut socket = sockets.get::<UdpSocket>(udpstate.handle);
                            match socket.send_slice(&udp_tx.data[..udp_tx.len as usize], endpoint) {
                                Ok(_) => buf.replace(NetMemResponse::Sent(udp_tx.len)).unwrap(),
                                _ => buf.replace(NetMemResponse::LibraryError).unwrap(),
                            }
                            // fire off a Pump to get the stack to actually transmit the ping; the send call merely queues it for sending
                            xous::try_send_message(
                                net_conn,
                                Message::new_scalar(
                                    Opcode::NetPump.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .ok();
                        } else {
                            buf.replace(NetMemResponse::Invalid).unwrap()
                        }
                    }
                    _ => buf.replace(NetMemResponse::Invalid).unwrap(),
                }
            }
            Some(Opcode::UdpSetTtl) => msg_scalar_unpack!(msg, ttl, port, _, _, {
                match udp_handles.get_mut(&(port as u16)) {
                    Some(udpstate) => {
                        let mut socket = sockets.get::<UdpSocket>(udpstate.handle);
                        let checked_ttl = if ttl > 255 || ttl == 0 { 64 } else { ttl as u8 };
                        socket.set_hop_limit(Some(checked_ttl));
                    }
                    None => {
                        log::error!(
                            "Set TTL message received, but no port was bound! port {} ttl {}",
                            port,
                            ttl
                        );
                    }
                }
            }),
            Some(Opcode::UdpGetTtl) => msg_blocking_scalar_unpack!(msg, port, _, _, _, {
                match udp_handles.get_mut(&(port as u16)) {
                    Some(udpstate) => {
                        let socket = sockets.get::<UdpSocket>(udpstate.handle);
                        let ttl = socket.hop_limit().unwrap_or(64); // 64 is the value used by smoltcp if hop limit isn't set
                        xous::return_scalar(msg.sender, ttl as usize).expect("couldn't return TTL");
                    }
                    None => {
                        log::error!(
                            "Set TTL message received, but no port was bound! port {}",
                            port
                        );
                        xous::return_scalar(msg.sender, usize::MAX).expect("couldn't return TTL");
                    }
                }
            }),

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
                                    let config = com
                                        .wlan_get_config()
                                        .expect("couldn't retrieve updated ipv4 config");
                                    log::info!("Network config acquired: {:?}", config);
                                    net_config = Some(config);
                                    let mac = EthernetAddress::from_bytes(&config.mac);

                                    // we need to clear the ARP cache in case we've migrated base stations (e.g. in a wireless network
                                    // that is coverd by multiple AP), as the host AP's MAC address would have changed, and we wouldn't
                                    // be able to route responses back. I can't seem to find a function in smoltcp 0.7.5 that allows us
                                    // to neatly clear the ARP cache as the BTreeMap that underlies it is moved into the container and
                                    // no "clear" API is exposed, so let's just rebuild the whole interface if we get a DHCP renewal.
                                    let neighbor_cache = NeighborCache::new(BTreeMap::new());
                                    let ip_addrs =
                                        [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)];
                                    let routes = Routes::new(BTreeMap::new());
                                    let device = device::NetPhy::new(&xns);
                                    let medium = device.capabilities().medium;
                                    let mut builder = InterfaceBuilder::new(device)
                                        .ip_addrs(ip_addrs)
                                        .routes(routes);
                                    if medium == Medium::Ethernet {
                                        builder = builder
                                            .ethernet_addr(mac)
                                            .neighbor_cache(neighbor_cache);
                                    }
                                    iface = builder.finalize();

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
            Some(Opcode::NetPump) => {
                let now = timer.elapsed_ms();
                let timestamp = Instant::from_millis(now as i64);
                match iface.poll(&mut sockets, timestamp) {
                    Ok(_) => {}
                    Err(e) => {
                        log::debug!("poll error: {}", e);
                    }
                }

                // this block handles TCP rx
                {
                    for (_connection, tcp_state) in tcp_handles.iter() {
                        if !tcp_state.shutdown_rx {
                            let mut socket = sockets.get::<TcpSocket>(tcp_state.handle);
                            if socket.can_recv() {
                                match socket.recv(|data| {
                                    let mut response = NetTcpResponse {
                                        len: data.len() as u16,
                                        data: [0; TCP_BUFFER_SIZE],
                                    };
                                    for (&src, dst) in data.iter().zip(response.data.iter_mut()) {
                                        *dst = src;
                                    }
                                    let buf = Buffer::into_buf(response)
                                        .expect("couldn't convert TCP response to memory message");
                                    buf.send(
                                        tcp_state.cid,
                                        NetTcpCallback::RxData.to_u32().unwrap(),
                                    )
                                    .expect("couldn't send TCP response");
                                    (data.len(), ())
                                }) {
                                    Ok(_) => (),
                                    Err(e) => match e {
                                        smoltcp::Error::Illegal => {
                                            log::warn!("TCP fast open not supported");
                                        }
                                        smoltcp::Error::Finished => {
                                            log::warn!("TCP packet received after close");
                                        }
                                        _ => {
                                            log::warn!("Unknown TCP error");
                                        }
                                    },
                                }
                            }
                        }
                    }
                }

                // Connect calls take time to establish. This block checks to see if connections
                // have been made and issues callbacks as necessary.
                for connection in tcp_connect_waiting.iter_mut() {
                    use smoltcp::socket::TcpState;
                    let socket;
                    let (env, _handle, fd, local_port, remote_port) = {
                        // If the connection is blank, or if it's still waiting to get
                        // connected, don't do anything.
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = sockets.get::<TcpSocket>(s.1);
                                if socket.state() == TcpState::SynSent
                                    || socket.state() == TcpState::SynReceived
                                {
                                    continue;
                                }
                            }
                        }
                        connection.take().unwrap()
                    };

                    log::trace!("tcp state is {:?}", socket.state());
                    if socket.state() == TcpState::Established {
                        respond_with_connected(env, fd, local_port, remote_port);
                    } else {
                        respond_with_error(env, NetError::TimedOut);
                    }
                }

                // This block handles TCP Rx for libstd callers
                for connection in tcp_rx_waiting.iter_mut() {
                    let mut socket;
                    let WaitingSocket {
                        mut env,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = sockets.get::<TcpSocket>(s.handle);
                                if !socket.can_recv() {
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

                    // If it can't receive, then the only explanation was that it timed out
                    if !socket.can_recv() {
                        respond_with_error(env, NetError::TimedOut);
                        continue;
                    }

                    let body = env.body.memory_message_mut().unwrap();
                    match socket.recv_slice(body.buf.as_slice_mut()) {
                        Ok(count) => {
                            body.valid = xous::MemorySize::new(count);
                        }
                        Err(e) => {
                            log::trace!("unable to receive: {:?}", e);
                            body.valid = None;
                        }
                    }
                }

                // This block handles TCP Tx for libstd callers
                for connection in tcp_tx_waiting.iter_mut() {
                    let mut socket;
                    let WaitingSocket {
                        mut env,
                        handle: _,
                        expiry: _,
                    } = {
                        match connection {
                            &mut None => continue,
                            Some(s) => {
                                socket = sockets.get::<TcpSocket>(s.handle);
                                if !socket.can_send() {
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

                    if !socket.can_send() {
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
                    body.valid = xous::MemorySize::new(sent_octets);
                    response_data[0] = 0;
                    response_data[1] = sent_octets as u32;
                }

                // this block handles TCP listeners
                // There is no lock on sending Rx messages to the listener as it is being transformed
                // because all messages come from *this thread* and by definition it is not re-entrant.
                // If you end up splitting the Tcp Rx task and the Tcp Listener tasks into separate threads,
                // Then you must worry about the possibility of a race condition between the transition of a
                // TcpListener to a regular TcpStream client (that is, an Rx packet forwarded before the
                // TcpStream is fully built).
                if tcp_listeners.len() > 0 {
                    // skip the whole chunk if we don't have any tcp listeners
                    log::trace!("checking {} TCP listener sockets", tcp_listeners.len());
                    for (&local_port, tcp_state_vec) in tcp_listeners.iter_mut() {
                        let mut remove_indices = Vec::<usize>::new();
                        for (index, tcp_state) in tcp_state_vec.iter().enumerate() {
                            let socket = sockets.get::<TcpSocket>(tcp_state.handle);
                            if socket.is_active() {
                                log::info!(
                                    "Promoting a Listener on port {:?} from {:?} to a Stream",
                                    socket.local_endpoint(),
                                    socket.remote_endpoint()
                                );
                                // 1. promote this socket to a TCP rx socket (this creates a double-entry that is cleaned up in step 3)
                                let connection = TcpConnection {
                                    remote: socket.remote_endpoint().addr,
                                    remote_port: socket.remote_endpoint().port,
                                    local_port,
                                };
                                tcp_handles.insert(connection, *tcp_state);

                                // 2. notify the listener
                                let note = NetTcpListenCallback {
                                    ip_addr: NetIpAddr::from(socket.remote_endpoint().addr),
                                    remote_port: socket.remote_endpoint().port,
                                    local_port,
                                };
                                log::debug!(
                                    "Listener active, notification sent to {}: {:x?}",
                                    tcp_state.cid,
                                    note
                                );
                                let buf =
                                    Buffer::into_buf(note).expect("can't transform memory message");
                                buf.send(
                                    tcp_state.cid,
                                    NetTcpCallback::ListenerActive.to_u32().unwrap(),
                                )
                                .expect("can't inform callback of active status");
                                log::trace!("listener index {} to remove", index);
                                remove_indices.push(index);
                                break;
                            } else {
                                log::trace!("socket not active: {:?}", socket.remote_endpoint());
                            }
                        }
                        // 3. now cleanup and remove the ports from the listeners status
                        remove_indices.sort(); // this should be in ascending order, but let's do this just to be sure.
                        remove_indices.reverse();
                        for index in remove_indices {
                            tcp_state_vec.remove(index);
                            log::trace!("removing listener index {}", index);
                        }
                        log::trace!("listeners remaining: {}", tcp_state_vec.len());
                        // 4. At this point, the Listener is no more. However, the caller side will "renew" the
                        // listener if the TcpListener is being used in an `incoming` loop.

                        // 5. Side note: an empty vector could be left in the tcp_listeners array. 🔥This is fine🔥.
                    }
                }

                // this block handles UDP
                {
                    for (port, udpstate) in udp_handles.iter() {
                        let handle = udpstate.handle;
                        let mut socket = sockets.get::<UdpSocket>(handle);
                        match socket.recv() {
                            Ok((data, endpoint)) => {
                                log::trace!(
                                    "udp:{} recv data: {:x?} from {}",
                                    port,
                                    data,
                                    endpoint
                                );
                                // return the data/endpoint tuple to the caller
                                let mut response = NetUdpResponse {
                                    endpoint_ip_addr: NetIpAddr::from(endpoint.addr),
                                    len: data.len() as u16,
                                    endpoint_port: endpoint.port,
                                    data: [0; UDP_RESPONSE_MAX_LEN],
                                };
                                for (&src, dst) in data.iter().zip(response.data.iter_mut()) {
                                    *dst = src;
                                }
                                let buf = Buffer::into_buf(response)
                                    .expect("couldn't convert UDP response to memory message");
                                buf.send(udpstate.cid, NetUdpCallback::RxData.to_u32().unwrap())
                                    .expect("couldn't send UDP response");
                                // now send copies to the cloned receiver array, if they exist
                                if let Some(clone_map) = udp_clones.get(port) {
                                    for &cids in clone_map.values() {
                                        let buf = Buffer::into_buf(response).expect(
                                            "couldn't convert UDP response to memory message",
                                        );
                                        buf.send(cids, NetUdpCallback::RxData.to_u32().unwrap())
                                            .expect("couldn't send UDP response");
                                    }
                                }
                            }
                            Err(_) => {
                                // do nothing
                            }
                        };
                    }
                }

                // this block contains the ICMP Rx handler. Tx is initiated by an incoming message to the Net crate.
                {
                    let mut socket = sockets.get::<IcmpSocket>(icmp_handle);
                    if !socket.is_open() {
                        log::error!("ICMP socket isn't open, something went wrong...");
                    }

                    if socket.can_recv() {
                        let (payload, _) = socket
                            .recv()
                            .expect("couldn't receive on socket despite asserting availability");
                        log::trace!("icmp payload: {:x?}", payload);
                        let now = timer.elapsed_ms();

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
                {
                    let now = timer.elapsed_ms();
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
                let timestamp = Instant::from_millis(timer.elapsed_ms() as i64);
                if let Some(delay) = iface.poll_delay(&sockets, timestamp) {
                    let delay_ms = delay.total_millis();
                    if delay_ms < 2 {
                        xous::try_send_message(
                            net_conn,
                            Message::new_scalar(Opcode::NetPump.to_usize().unwrap(), 0, 0, 0, 0),
                        )
                        .ok();
                    } else {
                        if delay_threads.load(Ordering::SeqCst) < MAX_DELAY_THREADS {
                            let prev_count = delay_threads.fetch_add(1, Ordering::SeqCst);
                            log::trace!(
                                "spawning checkup thread for {}ms. New total threads: {}",
                                delay_ms,
                                prev_count + 1
                            );
                            thread::spawn({
                                let parent_conn = net_conn.clone();
                                let delay_threads = delay_threads.clone();
                                move || {
                                    let tt = ticktimer_server::Ticktimer::new().unwrap();
                                    tt.sleep_ms(delay_ms as usize).unwrap();
                                    xous::try_send_message(
                                        parent_conn,
                                        Message::new_scalar(
                                            Opcode::NetPump.to_usize().unwrap(),
                                            0,
                                            0,
                                            0,
                                            0,
                                        ),
                                    )
                                    .ok();
                                    let prev_count = delay_threads.fetch_sub(1, Ordering::SeqCst);
                                    log::trace!(
                                        "terminating checkup thread. New total threads: {}",
                                        prev_count - 1
                                    );
                                }
                            });
                        } else {
                            log::warn!("Could not queue delay of {}ms in net stack due to thread exhaustion.", delay_ms);
                        }
                    }
                }
            }
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
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                // have to transform it through the local memory space because you can't re-lend pages
                let sub = buffer.to_original::<WifiStateSubscription, _>().unwrap();
                let buf = Buffer::into_buf(sub).expect("couldn't convert to memory message");
                buf.send(
                    cm_cid,
                    connection_manager::ConnectionManagerOpcode::SubscribeWifiStats
                        .to_u32()
                        .unwrap(),
                )
                .expect("couldn't forward subscription request");
            }
            Some(Opcode::UnsubWifiStats) => msg_blocking_scalar_unpack!(msg, s0, s1, s2, s3, {
                // now do something with the unsubscription
                let response = xous::send_message(
                    cm_cid,
                    Message::new_blocking_scalar(
                        connection_manager::ConnectionManagerOpcode::UnsubWifiStats
                            .to_usize()
                            .unwrap(),
                        s0,
                        s1,
                        s2,
                        s3,
                    ),
                )
                .expect("couldn't send unsub message");
                if let xous::Result::Scalar1(val) = response {
                    xous::return_scalar(msg.sender, val).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
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
                    .expect("couldn't send stop message");
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
                    .expect("couldn't send stop message");
                } else {
                    log::error!("Got incorrect start/stop code: {}", code);
                }
            }),
            Some(Opcode::Reset) => {
                net_config = None;
                let neighbor_cache = NeighborCache::new(BTreeMap::new());
                let ip_addrs = [IpCidr::new(Ipv4Address::UNSPECIFIED.into(), 0)];
                let routes = Routes::new(BTreeMap::new());
                let device = device::NetPhy::new(&xns);
                let medium = device.capabilities().medium;
                let mut builder = InterfaceBuilder::new(device)
                    .ip_addrs(ip_addrs)
                    .routes(routes);
                if medium == Medium::Ethernet {
                    builder = builder
                        .ethernet_addr(EthernetAddress::from_bytes(&[0; 6]))
                        .neighbor_cache(neighbor_cache);
                }
                iface = builder.finalize();
                iface.routes_mut().remove_default_ipv4_route();
                dns_allclear_hook.notify();
                // question: do we need to clear the UDP and ICMP states?
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
