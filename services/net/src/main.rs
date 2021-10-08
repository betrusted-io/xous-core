#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use num_traits::*;
use com::api::{Ipv4Conf, NET_MTU, ComIntSources};

/*
use smoltcp::iface::{InterfaceBuilder, NeighborCache};
use smoltcp::phy::{Loopback, Medium};
use smoltcp::socket::{SocketSet, TcpSocket, TcpSocketBuffer};
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
*/

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

    let mut net_config: Ipv4Conf; // = Ipv4Conf::default();
    let mut incoming_pkt_buf: [u8; NET_MTU] = [0; NET_MTU];
    let mut incoming_pkt: &mut [u8];

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
                            net_config = com.wlan_get_config().unwrap();
                            log::info!("Network config updated: {:?}", net_config);
                        },
                        ComIntSources::WlanRxReady => {
                            if let Some(rxlen) = maybe_rxlen {
                                incoming_pkt = &mut incoming_pkt_buf[0..rxlen as usize];
                                com.wlan_fetch_packet(incoming_pkt).unwrap();
                                log::info!("Rx: {:x?}", incoming_pkt);
                            } else {
                                log::error!("Got RxReady interrupt but no packet length specified!");
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
