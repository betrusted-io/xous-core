#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod ux;
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

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("my PID is {}", xous::process::id());

    // let's try keeping this completely private as a server. can we do that?
    let sid = xous::create_server().unwrap();
    ux::start_ux_thread();

    let _ = thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let boot_time = ClockValue::new(tt.elapsed_ms() as i64, 1000);

            let mut rng = ctap_crypto::rng256::XousRng256::new(&xns);
            // this call will block until the PDDB is mounted.
            let usb = usb_device_xous::UsbHid::new();
            let mut ctap_state = CtapState::new(&mut rng, check_user_presence, boot_time);
            let mut ctap_hid = CtapHid::new();
            loop {
                match usb.u2f_wait_incoming() {
                    Ok(msg) => {
                        log::trace!("FIDO listener got message: {:?}", msg);
                        let now = ClockValue::new(tt.elapsed_ms() as i64, 1000);
                        let reply = ctap_hid.process_hid_packet(&msg.packet, now, &mut ctap_state);
                        // This block handles sending packets.
                        for pkt_reply in reply {
                            let mut reply = FidoMsg::default();
                            reply.packet.copy_from_slice(&pkt_reply);
                            let status = usb.u2f_send(reply);
                            match status {
                                Ok(()) => {
                                    log::trace!("Sent U2F packet");
                                }
                                Err(e) => {
                                    log::error!("Error sending U2F packet: {:?}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("FIDO listener got an error: {:?}", e);
                    }
                }
            }
        }
    });

    let xns = xous_names::XousNames::new().unwrap();
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

fn check_user_presence(_cid: ChannelID) -> Result<(), Ctap2StatusCode> {
    log::warn!("check user presence called, but not implemented!");
    Ok(())
}
const KEEPALIVE_DELAY_MS: i64 = 100;
const KEEPALIVE_DELAY: Duration<i64> = Duration::from_ms(KEEPALIVE_DELAY_MS);
const SEND_TIMEOUT: Duration<i64> = Duration::from_ms(1000);

/*
// Returns whether the keepalive was sent, or false if cancelled.
fn send_keepalive_up_needed(
    cid: ChannelID,
    timeout: Duration<i64>,
) -> Result<(), Ctap2StatusCode> {
    let keepalive_msg = CtapHid::keepalive(cid, KeepaliveStatus::UpNeeded);
    for mut pkt in keepalive_msg {
        let status = usb_ctap_hid::send_or_recv_with_timeout(&mut pkt, timeout);
        match status {
            None => {
                #[cfg(feature = "debug_ctap")]
                writeln!(Console::new(), "Sending a KEEPALIVE packet timed out").unwrap();
                // TODO: abort user presence test?
            }
            Some(usb_ctap_hid::SendOrRecvStatus::Error) => panic!("Error sending KEEPALIVE packet"),
            Some(usb_ctap_hid::SendOrRecvStatus::Sent) => {
                #[cfg(feature = "debug_ctap")]
                writeln!(Console::new(), "Sent KEEPALIVE packet").unwrap();
            }
            Some(usb_ctap_hid::SendOrRecvStatus::Received) => {
                // We only parse one packet, because we only care about CANCEL.
                let (received_cid, processed_packet) = CtapHid::process_single_packet(&pkt);
                if received_cid != &cid {
                    #[cfg(feature = "debug_ctap")]
                    writeln!(
                        Console::new(),
                        "Received a packet on channel ID {:?} while sending a KEEPALIVE packet",
                        received_cid,
                    )
                    .unwrap();
                    return Ok(());
                }
                match processed_packet {
                    ProcessedPacket::InitPacket { cmd, .. } => {
                        if cmd == CtapHid::COMMAND_CANCEL {
                            // We ignore the payload, we can't answer with an error code anyway.
                            #[cfg(feature = "debug_ctap")]
                            writeln!(Console::new(), "User presence check cancelled").unwrap();
                            return Err(Ctap2StatusCode::CTAP2_ERR_KEEPALIVE_CANCEL);
                        } else {
                            #[cfg(feature = "debug_ctap")]
                            writeln!(
                                Console::new(),
                                "Discarded packet with command {} received while sending a KEEPALIVE packet",
                                cmd,
                            )
                            .unwrap();
                        }
                    }
                    ProcessedPacket::ContinuationPacket { .. } => {
                        #[cfg(feature = "debug_ctap")]
                        writeln!(
                            Console::new(),
                            "Discarded continuation packet received while sending a KEEPALIVE packet",
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_user_presence(cid: ChannelID) -> Result<(), Ctap2StatusCode> {
    // The timeout is N times the keepalive delay.
    const TIMEOUT_ITERATIONS: usize = ctap::TOUCH_TIMEOUT_MS as usize / KEEPALIVE_DELAY_MS as usize;

    /*
      This should do the following:
        - call send_keepalive_up_needed(cid, KEEPALIVE_DELAY) once every KEEPALIVE_DELAY interval
        - pop up a dialog box for TOUCH_TIMEOUT_MS asking to approve "something"
        - if TOUCH_TIMEOUT_MS has elapsed and no user approval is received:
           - return Err(Ctap2StatusCode::CTAP2_ERR_USER_ACTION_TIMEOUT)
        - else return Ok(())
    */
    send_keepalive_up_needed(cid, KEEPALIVE_DELAY)?;
    if false {
        Ok(())
    } else {
        Err(Ctap2StatusCode::CTAP2_ERR_USER_ACTION_TIMEOUT)
    }
}
*/