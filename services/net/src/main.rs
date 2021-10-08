#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use com::api::ComIntSources;
use num_traits::*;

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
    let llio = llio::Llio::new(&xns).unwrap();
    let net_cid = xous::connect(net_sid).unwrap();
    llio.hook_com_event_callback(Opcode::ComInterrupt.to_u32().unwrap(), net_cid).unwrap();
    // setup the interrupt masks
    let com = com::Com::new(&xns).unwrap();
    let mut com_int_list: Vec::<ComIntSources> = vec![];
    com.ints_get_active(&mut com_int_list);
    log::info!("COM initial interrupts: {:?}", com_int_list);
    com_int_list.clear();
    com_int_list.push(ComIntSources::WlanIpConfigUpdate);
    com_int_list.push(ComIntSources::WlanRxReady);
    com_int_list.push(ComIntSources::BatteryCritical);
    com.ints_enable(&com_int_list);
    com.ints_get_active(&mut com_int_list);
    log::info!("COM interrupts after enabling: {:?}", com_int_list);

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
                log::info!("COM got interrupts: {:?}", com_int_list);
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
