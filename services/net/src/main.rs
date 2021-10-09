#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use num_traits::*;
use com::api::{Ipv4Conf, NET_MTU, ComIntSources};

mod device;
use device::*;

use byteorder::{ByteOrder, NetworkEndian};
use std::cmp;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;

use smoltcp::phy::{Loopback, Medium, Device};
use smoltcp::iface::{InterfaceBuilder, NeighborCache, Routes, Interface};
use smoltcp::socket::{IcmpEndpoint, IcmpPacketMetadata, IcmpSocket, IcmpSocketBuffer, SocketSet};
use smoltcp::wire::{
    EthernetAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, IpCidr, Ipv4Address, Ipv4Cidr,
};
use smoltcp::{
    time::{Duration, Instant},
};

macro_rules! send_icmp_ping {
    ( $repr_type:ident, $packet_type:ident, $ident:expr, $seq_no:expr,
      $echo_payload:expr, $socket:expr, $remote_addr:expr ) => {{
        let icmp_repr = $repr_type::EchoRequest {
            ident: $ident,
            seq_no: $seq_no,
            data: &$echo_payload,
        };

        let icmp_payload = $socket.send(icmp_repr.buffer_len(), $remote_addr).unwrap();

        let icmp_packet = $packet_type::new_unchecked(icmp_payload);
        (icmp_repr, icmp_packet)
    }};
}

macro_rules! get_icmp_pong {
    ( $repr_type:ident, $repr:expr, $payload:expr, $waiting_queue:expr, $remote_addr:expr,
      $timestamp:expr, $received:expr ) => {{
        if let $repr_type::EchoReply { seq_no, data, .. } = $repr {
            if let Some(_) = $waiting_queue.get(&seq_no) {
                let packet_timestamp_ms = NetworkEndian::read_i64(data);
                println!(
                    "{} bytes from {}: icmp_seq={}, time={}ms",
                    data.len(),
                    $remote_addr,
                    seq_no,
                    $timestamp.total_millis() - packet_timestamp_ms
                );
                $waiting_queue.remove(&seq_no);
                $received += 1;
            }
        }
    }};
}

fn set_ipv4_addr<DeviceT>(iface: &mut Interface<'_, DeviceT>, cidr: Ipv4Cidr)
where
    DeviceT: for<'d> Device<'d>,
{
    iface.update_ip_addrs(|addrs| {
        let dest = addrs.iter_mut().next().unwrap();
        *dest = IpCidr::Ipv4(cidr);
    });
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let net_sid = xns.register_name(api::SERVER_NAME_NET, None).expect("can't register server");
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
    log::info!("COM initial pending interrupts: {:?}", com_int_list);
    com_int_list.clear();
    com_int_list.push(ComIntSources::WlanIpConfigUpdate);
    com_int_list.push(ComIntSources::WlanRxReady);
    com_int_list.push(ComIntSources::BatteryCritical);
    com.ints_enable(&com_int_list);
    com_int_list.clear();
    com.ints_get_active(&mut com_int_list);
    log::info!("COM pending interrupts after enabling: {:?}", com_int_list);

    let mut net_config: Option<Ipv4Conf> = None;
    let mut incoming_pkt_buf: [u8; NET_MTU] = [0; NET_MTU];
    let mut incoming_pkt: &mut [u8];

    // ping-specific storage
    let ping_remote_addr = IpAddress::from_str("10.0.245.10").expect("invalid address format");

    let icmp_rx_buffer = IcmpSocketBuffer::new(vec![IcmpPacketMetadata::EMPTY], vec![0; 256]);
    let icmp_tx_buffer = IcmpSocketBuffer::new(vec![IcmpPacketMetadata::EMPTY], vec![0; 256]);
    let icmp_socket = IcmpSocket::new(icmp_rx_buffer, icmp_tx_buffer);

    let mut sockets = SocketSet::new(vec![]);
    let icmp_handle = sockets.add(icmp_socket);

    let mut send_at = Instant::from_millis(0);
    let mut seq_no = 0;
    let mut received = 0;
    let mut echo_payload = [0xffu8; 40];
    let mut waiting_queue = HashMap::new();
    let ident = 0x22b;

    let count = 10; // number of ping iters
    let interval = Duration::from_secs(1);
    let timeout = Duration::from_secs(10);

    // link storage
    let neighbor_cache = NeighborCache::new(BTreeMap::new());
    let mut routes_storage = [None; 1];
    let routes = Routes::new(&mut routes_storage[..]);

    let device = device::NetPhy::new(&xns);
    let device_caps = device.capabilities();
    let medium = device.capabilities().medium;
    let mut builder = InterfaceBuilder::new(device)
        .ip_addrs([IpCidr::new(IpAddress::v4(0, 0, 0, 0), 24,)])
        .routes(routes);
    if medium == Medium::Ethernet {
        builder = builder
            .ethernet_addr(EthernetAddress::from_bytes(&[0; 6]))
            .neighbor_cache(neighbor_cache);
    }
    let mut iface = builder.finalize();

    log::trace!("ready to accept requests");
    // register a suspend/resume listener
    let sr_cid = xous::connect(net_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    loop {
        let msg = xous::receive_message(net_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
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
                            let config = com.wlan_get_config().unwrap();
                            net_config = Some(config);
                            log::info!("Network config updated: {:?}", config);
                            let mac = EthernetAddress::from_bytes(&config.mac);
                            iface.set_ethernet_addr(mac);
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

                            iface.routes_mut().remove_default_ipv4_route();
                            iface.routes_mut().add_default_ipv4_route(default_v4_gw).unwrap();
                        },
                        ComIntSources::WlanRxReady => {
                            if let Some(_config) = net_config {
                                if let Some(rxlen) = maybe_rxlen {
                                    match iface.device_mut().push_rx_avail(rxlen) {
                                        None => {},
                                        Some(_) => log::warn!("Got more packets, but smoltcp didn't receive them"),
                                    }

                                    // now fire off the smoltcp receive machine....
                                    {
                                        let timestamp = Instant::now();
                                        match iface.poll(&mut sockets, timestamp) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                log::debug!("poll error: {}", e);
                                            }
                                        }
                                        {
                                            let timestamp = Instant::now();
                                            let mut socket = sockets.get::<IcmpSocket>(icmp_handle);
                                            if !socket.is_open() {
                                                socket.bind(IcmpEndpoint::Ident(ident)).unwrap();
                                                send_at = timestamp;
                                            }

                                            if socket.can_send() && seq_no < count as u16 && send_at <= timestamp {
                                                NetworkEndian::write_i64(&mut echo_payload, timestamp.total_millis());

                                                match ping_remote_addr {
                                                    IpAddress::Ipv4(_) => {
                                                        let (icmp_repr, mut icmp_packet) = send_icmp_ping!(
                                                            Icmpv4Repr,
                                                            Icmpv4Packet,
                                                            ident,
                                                            seq_no,
                                                            echo_payload,
                                                            socket,
                                                            ping_remote_addr
                                                        );
                                                        icmp_repr.emit(&mut icmp_packet, &device_caps.checksum);
                                                    }
                                                    _ => unimplemented!(),
                                                }

                                                waiting_queue.insert(seq_no, timestamp);
                                                seq_no += 1;
                                                send_at += interval;
                                            }

                                            if socket.can_recv() {
                                                let (payload, _) = socket.recv().unwrap();

                                                match ping_remote_addr {
                                                    IpAddress::Ipv4(_) => {
                                                        let icmp_packet = Icmpv4Packet::new_checked(&payload).unwrap();
                                                        let icmp_repr =
                                                            Icmpv4Repr::parse(&icmp_packet, &device_caps.checksum).unwrap();
                                                        get_icmp_pong!(
                                                            Icmpv4Repr,
                                                            icmp_repr,
                                                            payload,
                                                            waiting_queue,
                                                            ping_remote_addr,
                                                            timestamp,
                                                            received
                                                        );
                                                    }
                                                    _ => unimplemented!(),
                                                }
                                            }

                                            waiting_queue.retain(|seq, from| {
                                                if timestamp - *from < timeout {
                                                    true
                                                } else {
                                                    log::info!("From {} icmp_seq={} timeout", ping_remote_addr, seq);
                                                    false
                                                }
                                            });

                                            if seq_no == count as u16 && waiting_queue.is_empty() {
                                                break;
                                            }
                                        }
                                    }

                                    /*
                                    incoming_pkt = &mut incoming_pkt_buf[0..rxlen as usize];
                                    com.wlan_fetch_packet(incoming_pkt).unwrap();
                                    log::info!("Rx: {:x?}", incoming_pkt);*/
                                } else {
                                    log::error!("Got RxReady interrupt but no packet length specified!");
                                }
                            }
                        },
                        ComIntSources::WlanSsidScanDone => {

                        },
                        _ => {
                            log::error!("Invalid interrupt type received");
                        }
                    }
                }
                com.ints_ack(&com_int_list);
            }
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                // handle an suspend/resume state stuff here. right now, it's a NOP
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
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
