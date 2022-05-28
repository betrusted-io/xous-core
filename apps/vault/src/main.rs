#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod repl;
use repl::*;
use num_traits::*;
use xous_ipc::Buffer;
use usbd_human_interface_device::device::fido::*;
use std::thread;

mod ctap;
use ctap::hid::{ChannelID, CtapHid, KeepaliveStatus, ProcessedPacket};
use ctap::status_code::Ctap2StatusCode;
use ctap::CtapState;
mod shims;
use shims::*;

/*
UI concept:

  |-----------------|
  |                 |
  | List view       |
  | area            |
  |                 |
  |                 |
  |                 |
  |                 |
  |                 |
  |-----------------|
  | List filter     |
  |-----------------|
  |F1 | F2 | F3 | F4|
  |-----------------|

  F1-F4: switch between functions using F-keys. Functions are:
    - FIDO2   (U2F authenicators)
    - TOTP    (time based authenticators)
    - Vault   (passwords)
    - Prefs   (preferences)
  Tap once to switch to the sub-function.
  Once on the sub-function, tap the corresponding F-key again to raise
  the menu for that sub-function.

  List filter:
    - Any regular keys hit here appear in the search input. It automatically
      filters the content in the list view area to the set of strings that match
      the search input

  Up/down arrow: picks a list view item
  Left/right arrow: moves up or down the list view in pages
  Enter: picks the selected list view
  Select: *alaways* raises system 'main menu'
 */

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// redraw our UI
    Redraw,
    /// change focus
    ChangeFocus,
    /// exit the application
    Quit,
}

// This name should be (1) unique (2) under 64 characters long and (3) ideally descriptive.
pub(crate) const SERVER_NAME_VAULT: &str = "Key Vault";

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // let's try keeping this completely private as a server. can we do that?
    let sid = xous::create_server().unwrap();

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

    let rng = ctap_ctap_crypto::rng256::XousRng256::new(&xns);
    let mut ctap_state = CtapState::new(&mut rng, check_user_presence, boot_time);
    //let mut ctap_hid = CtapHid::new();

    let mut repl = Repl::new(&xns, sid);
    let mut update_repl = true;
    let mut was_callback = false;
    let mut allow_redraw = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::debug!("got message {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Line) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::trace!("repl got input line: {}", s.as_str());
                repl.input(s.as_str()).expect("Vault couldn't accept input string");
                update_repl = true; // set a flag, instead of calling here, so message can drop and calling server is released
                was_callback = false;
            }
            Some(VaultOp::Redraw) => {
                if allow_redraw {
                    repl.redraw().expect("Vault couldn't redraw");
                }
            }
            Some(VaultOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        allow_redraw = true;
                    }
                }
            }),
            Some(VaultOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            _ => {
                log::trace!("got unknown message, treating as callback");
                repl.msg(msg);
                update_repl = true;
                was_callback = true;
            }
        }
        if update_repl {
            repl.update(was_callback);
            update_repl = false;
        }
        log::trace!("reached bottom of main loop");
    }
    // clean up our program
    log::error!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

/*

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

*/