#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use num_traits::*;
use com::api::{ComIntSources, Ipv4Conf, NET_MTU};

mod device;

use xous::{send_message, Message, CID, SID, msg_scalar_unpack, msg_blocking_scalar_unpack};
use xous_ipc::Buffer;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryInto;

use smoltcp::phy::{Medium, Device};
use smoltcp::iface::{InterfaceBuilder, NeighborCache, Routes, Interface};
use smoltcp::socket::{IcmpEndpoint, IcmpPacketMetadata, IcmpSocket, IcmpSocketBuffer, SocketSet};
use smoltcp::wire::{
    EthernetAddress, IpAddress, IpCidr, Ipv4Address, Ipv4Cidr, IpEndpoint
};
use smoltcp::wire::{
    Icmpv4Packet, Icmpv4Repr, Icmpv6Packet, Icmpv6Repr,
};
use byteorder::{ByteOrder, NetworkEndian};

use smoltcp::socket::{UdpPacketMetadata, UdpSocket, UdpSocketBuffer, SocketHandle};
use smoltcp::time::Instant;
use std::thread;
use std::sync::Arc;
use core::sync::atomic::{AtomicU32, Ordering};

const PING_DEFAULT_TIMEOUT_MS: u32 = 10_000;

fn set_ipv4_addr<DeviceT>(iface: &mut Interface<'_, DeviceT>, cidr: Ipv4Cidr)
where
    DeviceT: for<'d> Device<'d>,
{
    iface.update_ip_addrs(|addrs| {
        let dest = addrs.iter_mut().next().expect("trouble updating ipv4 addresses in routing table");
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

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let net_sid = xns.register_name(api::SERVER_NAME_NET, None).expect("can't register server");
    let net_conn = xous::connect(net_sid).unwrap();
    log::trace!("registered with NS -- {:?}", net_sid);

    // hook the COM interrupt listener
    let mut llio = llio::Llio::new(&xns).unwrap();
    let net_cid = xous::connect(net_sid).unwrap();
    llio.hook_com_event_callback(Opcode::ComInterrupt.to_u32().unwrap(), net_cid).unwrap();
    llio.com_event_enable(true).unwrap();
    // setup the interrupt masks
    let com = com::Com::new(&xns).unwrap();
    let mut com_int_list: Vec::<ComIntSources> = vec![];
    com.ints_get_active(&mut com_int_list);
    log::debug!("COM initial pending interrupts: {:?}", com_int_list);
    com_int_list.clear();
    com_int_list.push(ComIntSources::WlanIpConfigUpdate);
    com_int_list.push(ComIntSources::WlanRxReady);
    com_int_list.push(ComIntSources::BatteryCritical);
    com.ints_enable(&com_int_list);
    com_int_list.clear();
    com.ints_get_active(&mut com_int_list);
    log::debug!("COM pending interrupts after enabling: {:?}", com_int_list);
    const MAX_DELAY_THREADS: u32 = 10; // limit the number of concurrent delay threads. Typically we have 1-2 running at any time, but DoS conditions could lead to many more.
    let delay_threads = Arc::new(AtomicU32::new(0));
    let mut net_config: Option<Ipv4Conf> = None;

    // storage for all our sockets
    let mut sockets = SocketSet::new(vec![]);

    // ping storage
    // up to four concurrent pings in the queue
    let icmp_rx_buffer = IcmpSocketBuffer::new(
        vec![
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY
            ],
            vec![0; 1024]);
    let icmp_tx_buffer = IcmpSocketBuffer::new(
        vec![
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY,
                IcmpPacketMetadata::EMPTY
            ],
            vec![0; 1024]);
    let mut icmp_socket = IcmpSocket::new(icmp_rx_buffer, icmp_tx_buffer);
    let ident = 0x22b;
    icmp_socket.bind(IcmpEndpoint::Ident(ident)).expect("couldn't bind to icmp socket");
    let icmp_handle = sockets.add(icmp_socket);
    let mut seq: u16 = 0;
    // this record stores the origin time + IP address of the outgoing ping sequence number
    let mut ping_destinations = HashMap::<PingConnection, HashMap::<u16, u64>>::new();
    let mut ping_timeout_ms = PING_DEFAULT_TIMEOUT_MS;

    // udp storage
    let mut udp_handles = HashMap::<u16, UdpState>::new();
    // UDP requires multiple copies. The way it works is that Tx can come from anyone;
    // for Rx, copies of a CID,SID tuple are kept for every clone is kept in a HashMap. This
    // allows for the Rx data to be cc:'d to each clone, and identified by SID upon drop
    let mut udp_clones = HashMap::<u16, HashMap::<[u32; 4], CID>>::new(); // additional clones for UDP responders

    // other link storage
    let timer = ticktimer_server::Ticktimer::new().unwrap();
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
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut cid_to_disconnect: Option<CID> = None;
    loop {
        let mut msg = xous::receive_message(net_sid).unwrap();
        if let Some(dc_cid) = cid_to_disconnect.take() { // disconnect previous loop iter's connection after d/c OK response was sent
            unsafe{
                match xous::disconnect(dc_cid) {
                   Ok(_) => {},
                   Err(xous::Error::ServerNotFound) => {
                       log::trace!("Disconnect returned the expected error code for a remote that has been destroyed.")
                   },
                   Err(e) => {
                       log::error!("Attempt to de-allocate CID to destroyed server met with error: {:?}", e);
                   },
                }
            }
        }
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Ping) => {
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
                let mut pkt = buf.to_original::<NetPingPacket, _>().unwrap();
                let mut socket = sockets.get::<IcmpSocket>(icmp_handle);
                if socket.can_send() {
                    log::trace!("sending ping to {:?}", pkt.endpoint);
                    let remote = IpAddress::from(pkt.endpoint);
                    // we take advantage of the fact that the same CID is always returned for repeated connect requests to the same SID.
                    let cid = match pkt.server {
                        XousServerId::PrivateSid(sid) => match xous::connect(SID::from_array(sid)) {
                            Ok(cid) => cid,
                            Err(e) => {
                                log::error!("Ping request with single-use callback SID is invalid. Aborting request. {:?}",e);
                                continue;
                            }
                        }
                        XousServerId::ServerName(name) => match xns.request_connection(name.to_str()) {
                            Ok(cid) => cid,
                            Err(e) => {
                                log::error!("Ping request received, but callback name '{}' is invalid. Aborting request. {:?}", name, e);
                                continue;
                            }
                        }
                    };
                    // this structure can be a HashMap key because it "should" be invariant across well-formed ping requests
                    let conn = PingConnection {
                        remote,
                        cid,
                        retop: pkt.return_opcode,
                    };
                    log::trace!("ping conn info: remote {:?} / cid: {} / retp: {}", remote, cid, pkt.return_opcode);
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
                    xous::try_send_message(net_conn,
                        Message::new_scalar(
                            Opcode::NetPump.to_usize().unwrap(),
                            0, 0, 0, 0)
                    ).ok();
                    pkt.sent_ok = Some(true);
                } else {
                    pkt.sent_ok = Some(false);
                }
                buf.replace(pkt).expect("Xous couldn't issue response to Ping request");
            }
            Some(Opcode::PingSetTtl) => msg_scalar_unpack!(msg, ttl, _, _, _, {
                let checked_ttl = if ttl > 255 {
                    255 as u8
                } else {
                    ttl as u8
                };
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
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
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
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
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
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
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
            Some(Opcode::UdpBind) => {
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
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
                            }, // the same SID has double-registered, this is an error
                            None => buf.replace(NetMemResponse::Ok).unwrap()
                        }
                    } else {
                        // otherwise, create the clone mapping entry
                        let mut newmap = HashMap::new();
                        newmap.insert(sid, cid);
                        udp_clones.insert(
                            udpspec.port,
                            newmap
                        );
                    }
                    buf.replace(NetMemResponse::Ok).unwrap();
                } else {
                    let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; buflen]);
                    let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketMetadata::EMPTY], vec![0; buflen]);
                    let mut udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);
                    match udp_socket.bind(udpspec.port) {
                        Ok(_) => {
                            let sid = SID::from_array(udpspec.cb_sid);
                            let udpstate = UdpState {
                                handle: sockets.add(udp_socket),
                                cid: xous::connect(sid).unwrap(),
                                sid
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
            },
            Some(Opcode::UdpClose) => {
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
                let udpspec = buf.to_original::<NetUdpBind, _>().unwrap();
                // need to find the SID that matches either in the clone array, or the primary binding.
                // first check the clone array, then fall back to the primary binding
                match udp_clones.get_mut(&udpspec.port) {
                    Some(clone_map) => {
                        match clone_map.remove(&udpspec.cb_sid) {
                            Some(cid) => {
                                cid_to_disconnect = Some(cid);
                                buf.replace(NetMemResponse::Ok).unwrap();
                                continue;
                            }
                            None => {}
                        }
                    }
                    None => {}
                }
                match udp_handles.remove(&udpspec.port) {
                    Some(udpstate) => {
                        if udpstate.sid == SID::from_array(udpspec.cb_sid) {
                            match udp_clones.get_mut(&udpspec.port) {
                                // if the clone map is nil, close the socket, we're done
                                None => {
                                    sockets.get::<UdpSocket>(udpstate.handle).close();
                                    buf.replace(NetMemResponse::Ok).unwrap();
                                }
                                // if the clone map has entries, promote an arbitrary map entry to the primary handle
                                Some(clone_map) => {
                                    if clone_map.len() == 0 {
                                        // removing SIDs doesn't remove the map, so it's possible to have an empty mapping. Get rid of it, and we're done.
                                        udp_clones.remove(&udpspec.port);
                                        sockets.get::<UdpSocket>(udpstate.handle).close();
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
                    _ => {
                        buf.replace(NetMemResponse::Invalid).unwrap()
                    }
                }
            },
            Some(Opcode::UdpTx) => {
                use std::convert::TryInto;
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
                let udp_tx = buf.to_original::<NetUdpTransmit, _>().unwrap();
                match udp_handles.get_mut(&udp_tx.local_port) {
                    Some(udpstate) => {
                        if let Some(dest_socket) = udp_tx.dest_socket {
                            let endpoint = IpEndpoint::new(
                                dest_socket.addr.try_into().unwrap(),
                                dest_socket.port
                            );
                            let mut socket = sockets.get::<UdpSocket>(udpstate.handle);
                            match socket.send_slice(&udp_tx.data[..udp_tx.len as usize], endpoint) {
                                Ok(_) => buf.replace(NetMemResponse::Sent(udp_tx.len)).unwrap(),
                                _ => buf.replace(NetMemResponse::LibraryError).unwrap(),
                            }
                            // fire off a Pump to get the stack to actually transmit the ping; the send call merely queues it for sending
                            xous::try_send_message(net_conn,
                                Message::new_scalar(
                                    Opcode::NetPump.to_usize().unwrap(),
                                    0, 0, 0, 0)
                            ).ok();
                        } else {
                            buf.replace(NetMemResponse::Invalid).unwrap()
                        }
                    }
                    _ => buf.replace(NetMemResponse::Invalid).unwrap()
                }
            },
            Some(Opcode::UdpSetTtl) => msg_scalar_unpack!(msg, ttl, port, _, _, {
                match udp_handles.get_mut(&(port as u16)) {
                    Some(udpstate) => {
                        let mut socket = sockets.get::<UdpSocket>(udpstate.handle);
                        let checked_ttl = if ttl > 255 || ttl == 0 {
                            64
                        } else {
                            ttl as u8
                        };
                        socket.set_hop_limit(Some(checked_ttl));
                    }
                    None => {
                        log::error!("Set TTL message received, but no port was bound! port {} ttl {}", port, ttl);
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
                        log::error!("Set TTL message received, but no port was bound! port {}", port);
                        xous::return_scalar(msg.sender, usize::MAX).expect("couldn't return TTL");
                    }
                }
            }),

            Some(Opcode::ComInterrupt) => {
                com_int_list.clear();
                let maybe_rxlen = com.ints_get_active(&mut com_int_list);
                log::debug!("COM got interrupts: {:?}, {:?}", com_int_list, maybe_rxlen);
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
                        },
                        ComIntSources::WlanIpConfigUpdate => {
                            // right now the WLAN implementation only does IPV4. So IPV6 compatibility ends here.
                            // if IPV6 gets added to the EC/COM bus, ideally this is one of a couple spots in Xous that needs a tweak.
                            let config = com.wlan_get_config().expect("couldn't retrieve updated ipv4 config");
                            log::info!("Network config acquired: {:?}", config);
                            net_config = Some(config);
                            let mac = EthernetAddress::from_bytes(&config.mac);

                            // we need to clear the ARP cache in case we've migrated base stations (e.g. in a wireless network
                            // that is coverd by multiple AP), as the host AP's MAC address would have changed, and we wouldn't
                            // be able to route responses back. I can't seem to find a function in smoltcp 0.7.5 that allows us
                            // to neatly clear the ARP cache as the BTreeMap that underlies it is moved into the container and
                            // no "clear" API is exposed, so let's just rebuild the whole interface if we get a DHCP renewal.
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
                                    .ethernet_addr(mac)
                                    .neighbor_cache(neighbor_cache);
                            }
                            iface = builder.finalize();

                            let ip_addr =
                                Ipv4Cidr::new(Ipv4Address::new(
                                    config.addr[0],
                                    config.addr[1],
                                    config.addr[2],
                                    config.addr[3],
                                ), 24);
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
                                Ok(route) => log::info!("routing table updated successfully [{:?}]", route),
                                Err(e) => log::error!("routing table update error: {}", e),
                            }
                            dns_allclear_hook.notify();
                            dns_ipv4_hook.notify_custom_args([
                                Some(u32::from_be_bytes(config.dns1)),
                                None, None, None,
                            ]);
                            // the current implementation always returns 0.0.0.0 as the second dns,
                            // ignore this if that's what we've got; otherwise, pass it on.
                            if config.dns2 != [0, 0, 0, 0] {
                                dns_ipv4_hook.notify_custom_args([
                                    Some(u32::from_be_bytes(config.dns2)),
                                    None, None, None,
                                ]);
                            }
                        },
                        ComIntSources::WlanRxReady => {
                            if let Some(_config) = net_config {
                                if let Some(rxlen) = maybe_rxlen {
                                    match iface.device_mut().push_rx_avail(rxlen) {
                                        None => {} //log::info!("pushed {} bytes avail to iface", rxlen),
                                        Some(_) => log::warn!("Got more packets, but smoltcp didn't drain them in time"),
                                    }
                                    send_message(
                                        net_conn,
                                        Message::new_scalar(Opcode::NetPump.to_usize().unwrap(), 0, 0, 0, 0)
                                    ).expect("WlanRxReady couldn't pump the loop");
                                } else {
                                    log::error!("Got RxReady interrupt but no packet length specified!");
                                }
                            }
                        },
                        ComIntSources::WlanSsidScanDone => {
                            log::info!("got ssid scan done");
                        },
                        _ => {
                            log::error!("Invalid interrupt type received");
                        }
                    }
                }
                com.ints_ack(&com_int_list);
            }
            Some(Opcode::NetPump) => {
                let timestamp = Instant::from_millis(timer.elapsed_ms() as i64);
                match iface.poll(&mut sockets, timestamp) {
                    Ok(_) => { }
                    Err(e) => {
                        log::debug!("poll error: {}", e);
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
                                let buf = Buffer::into_buf(response).expect("couldn't convert UDP response to memory message");
                                buf.send(udpstate.cid, NetUdpCallback::RxData.to_u32().unwrap()).expect("couldn't send UDP response");
                                // now send copies to the cloned receiver array, if they exist
                                if let Some(clone_map) = udp_clones.get(port) {
                                    for &cids in clone_map.values() {
                                        let buf = Buffer::into_buf(response).expect("couldn't convert UDP response to memory message");
                                        buf.send(cids, NetUdpCallback::RxData.to_u32().unwrap()).expect("couldn't send UDP response");
                                    }
                                }
                            }
                            Err(_) => {
                                // do nothing
                            },
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
                        let (payload, _) = socket.recv().expect("couldn't receive on socket despite asserting availability");
                        log::trace!("icmp payload: {:x?}", payload);
                        let now = timer.elapsed_ms();

                        for (connection, waiting_queue) in ping_destinations.iter_mut() {
                            let remote_addr = connection.remote;
                            match remote_addr {
                                IpAddress::Ipv4(_) => {
                                    let icmp_packet = Icmpv4Packet::new_checked(&payload).unwrap();
                                    let icmp_repr =
                                        Icmpv4Repr::parse(&icmp_packet, &device_caps.checksum).unwrap();
                                    if let Icmpv4Repr::EchoReply { seq_no, data, .. } = icmp_repr {
                                        log::trace!("got icmp seq no {} / data: {:x?}", seq_no, data);
                                        if let Some(_) = waiting_queue.get(&seq_no) {
                                            let packet_timestamp_ms = NetworkEndian::read_i64(data);
                                            waiting_queue.remove(&seq_no);
                                            // use try_send_message because we don't want to block if the recipient's queue is full;
                                            // instead, the message is just dropped
                                            match xous::try_send_message(connection.cid,
                                                Message::new_scalar(
                                                    connection.retop,
                                                    NetPingCallback::NoErr.to_usize().unwrap(),
                                                    u32::from_be_bytes(remote_addr.as_bytes().try_into().unwrap()) as usize,
                                                    seq_no as usize,
                                                    (now as i64 - packet_timestamp_ms) as usize,
                                                )
                                            ) {
                                                Ok(_) => {},
                                                Err(xous::Error::ServerQueueFull) => {
                                                    log::warn!("Got seq {} response, but upstream server queue is full; dropping.", &seq_no);
                                                },
                                                Err(e) => {
                                                    log::error!("Unhandled error: {:?}; ignoring", e);
                                                }
                                            }
                                        }
                                    } else if let Icmpv4Repr::DstUnreachable { reason, header, .. } = icmp_repr {
                                        log::warn!("Got dst unreachable {:?}: {:?}", header.dst_addr, reason);
                                        let reason_code: u8 = From::from(reason);
                                        match xous::try_send_message(connection.cid,
                                            Message::new_scalar(
                                                connection.retop,
                                                NetPingCallback::Unreachable.to_usize().unwrap() | (reason_code as usize) << 24,
                                                u32::from_be_bytes(remote_addr.as_bytes().try_into().unwrap()) as usize,
                                                0,
                                                0,
                                            )
                                        ) {
                                            Ok(_) => {},
                                            Err(xous::Error::ServerQueueFull) => {
                                                log::warn!("Got dst {:?} unreachable, but upstream server queue is full; dropping.", remote_addr);
                                            },
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
                                            match xous::try_send_message(connection.cid,
                                                Message::new_scalar(
                                                    connection.retop,
                                                    NetPingCallback::NoErr.to_usize().unwrap(),
                                                    u32::from_be_bytes(ra[..4].try_into().unwrap()) as usize,
                                                    u32::from_be_bytes(ra[12..].try_into().unwrap()) as usize,
                                                    (now as i64 - packet_timestamp_ms) as usize,
                                                )
                                            ) {
                                                Ok(_) => {},
                                                Err(xous::Error::ServerQueueFull) => {
                                                    log::warn!("Got seq {} response, but upstream server queue is full; dropping.", &seq_no);
                                                },
                                                Err(e) => {
                                                    log::error!("Unhandled error: {:?}; ignoring", e);
                                                }
                                            }
                                        }
                                    } else if let Icmpv6Repr::DstUnreachable { reason, header, .. } = icmp_repr {
                                        let reason_code: u8 = From::from(reason);
                                        log::warn!("Got dst unreachable {:?}: {:?}", header.dst_addr, reason);
                                        match xous::try_send_message(connection.cid,
                                            Message::new_scalar(
                                                connection.retop,
                                                NetPingCallback::Unreachable.to_usize().unwrap() | (reason_code as usize) << 24,
                                                u32::from_be_bytes(ra[..4].try_into().unwrap()) as usize,
                                                u32::from_be_bytes(ra[8..12].try_into().unwrap()) as usize,
                                                u32::from_be_bytes(ra[12..].try_into().unwrap()) as usize,
                                            )
                                        ){
                                            Ok(_) => {},
                                            Err(xous::Error::ServerQueueFull) => {
                                                log::warn!("Got dst {:?} unreachable, but upstream server queue is full; dropping.", remote_addr);
                                            },
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
                        xous::try_send_message(net_conn,
                            Message::new_scalar(
                                Opcode::NetPump.to_usize().unwrap(),
                                0, 0, 0, 0)
                        ).ok();
                    } else {
                        if delay_threads.load(Ordering::SeqCst) < MAX_DELAY_THREADS {
                            let prev_count = delay_threads.fetch_add(1, Ordering::SeqCst);
                            log::trace!("spawning checkup thread for {}ms. New total threads: {}", delay_ms, prev_count + 1);
                            thread::spawn({
                                let parent_conn = net_conn.clone();
                                let delay_threads = delay_threads.clone();
                                move || {
                                    let tt = ticktimer_server::Ticktimer::new().unwrap();
                                    tt.sleep_ms(delay_ms as usize).unwrap();
                                    xous::try_send_message(parent_conn,
                                        Message::new_scalar(
                                            Opcode::NetPump.to_usize().unwrap(),
                                            0, 0, 0, 0)
                                    ).ok();
                                    let prev_count = delay_threads.fetch_sub(1, Ordering::SeqCst);
                                    log::trace!("terminating checkup thread. New total threads: {}", prev_count - 1);
                                }
                            });
                        } else {
                            log::warn!("Could not queue delay of {}ms in net stack due to thread exhaustion.", delay_ms);
                        }
                    }
                }
            }
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                // handle an suspend/resume state stuff here. right now, it's a NOP
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
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
    xns.unregister_server(net_sid).unwrap();
    xous::destroy_server(net_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
