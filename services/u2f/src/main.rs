#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

mod ctap;
use ctap::hid::{ChannelID, CtapHid, KeepaliveStatus, ProcessedPacket};
use ctap::status_code::Ctap2StatusCode;
use ctap::CtapState;

use num_traits::*;
use std::thread;
use usbd_human_interface_device::device::fido::*;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // TODO: figure out what, if any, should be the limit of connections to the U2F server?
    let u2f_sid = xns.register_name(api::SERVER_NAME_U2F, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", u2f_sid);

    let _ = thread::spawn({
        move || {
            let usb = usb_device_xous::UsbHid::new();
            loop {
                match usb.u2f_wait_incoming() {
                    Ok(msg) => {
                        log::info!("FIDO listener got message: {:?}", msg);
                    }
                    Err(e) => {
                        log::warn!("FIDO listener got an error: {:?}", e);
                    }
                }
            }
        }
    });

    let mut ctap_state = CtapState::new(&mut rng, check_user_presence, boot_time);
    let mut ctap_hid = CtapHid::new();

    loop {
        let msg = xous::receive_message(u2f_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Quit) => {
                log::warn!("Quit received, goodbye world!");
                break;
            },
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(u2f_sid).unwrap();
    xous::destroy_server(u2f_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
