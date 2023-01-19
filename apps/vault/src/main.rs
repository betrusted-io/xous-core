#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod ux;
use ux::*;
use num_traits::*;
use xous_ipc::Buffer;
use xous::{send_message, Message};
use usbd_human_interface_device::device::fido::*;
use std::thread;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

mod ctap;
use ctap::hid::{ChannelID, CtapHid};
use ctap::status_code::Ctap2StatusCode;
use ctap::CtapState;
mod shims;
use shims::*;
mod submenu;
mod actions;
mod totp;
mod prereqs;
mod vendor_commands;
mod storage;

use locales::t;

use framework::ListItem;
use actions::{ActionOp, start_actions_thread};

use crate::prereqs::ntp_updater;
use crate::vendor_commands::VendorSession;
use std::collections::BTreeMap;

// CTAP2 testing notes:
// run our branch and use this to forward the prompts on to the device:
// netcat -k -u -l 6502 > /dev/ttyS0
// use the "autotest" feature to remove some excess prompts that interfere with the test

// the OpenSK code is based off of commit f2496a8e6d71a4e838884996a1c9b62121f87df2 from the
// Google OpenSK repository. The last push was Nov 19 2021, and the initial merge into Xous
// was finished on June 9 2022. Any patches to this code base will have to be manually
// applied. Please update the information here to reflect the latest patch status.

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

  Organization:
    - Main thread (vaultux object): "responsive" UI operations - must always be able to respond to redraw commands.
      operates on lists of data shared between main & actions thread
    - Actions thread (actions object): "blocking" UI operations - manages multi-sequence dialog queries, database access
    - Fido thread: handles USB interactions. Can always pop up a dialog box, but it cannot override a dialog-in-progress.
    - Icontray thread: a simple server that serves as a shim between the IME structure and this to create an icontray function
 */

 // TOTP test I65VU7K5ZQL7WB4E https://authenticationtest.com/totpChallenge/
 // otpauth-migration://offline?data=Ci8KCke7Wn1dzBf7B4QSG3RvdHBAYXV0aGVudGljYXRpb250ZXN0LmNvbSABKAEwAhABGAEgACjh8Yv%2B%2BP%2F%2F%2F%2F8B
/*
otp_parameters {
  secret: "G\273Z}]\314\027\373\007\204"
  name: "totp@authenticationtest.com"
  algorithm: SHA1
  digits: SIX
  type: TOTP
}
version: 1
batch_size: 1
batch_index: 0
batch_id: -1883047711

>>> b32decode("I65VU7K5ZQL7WB4E").hex()
'47bb5a7d5dcc17fb0784'

To clear test entries:
  pddb dictdelete vault.passwords
  pddb dictdelete vault.totp
  pddb dictdelete fido.cred
  pddb dictdelete fido.u2fapps

*/

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum VaultOp {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// incremental line of text
    IncrementalLine,
    /// redraw our UI
    Redraw,
    /// ignore dirty rectangles and redraw everything
    FullRedraw,
    /// reload the database (slow), and ignore dirty rectangles and redraw everything
    ReloadDbAndFullRedraw,
    /// change focus
    ChangeFocus,

    /// Partial menu
    MenuChangeFont,
    MenuDeleteStage1,
    MenuEditStage1,
    MenuAutotype,
    MenuReadoutMode,

    /// PDDB basis change
    BasisChange,

    /// Nop while waiting for prerequisites to be filled
    Nop,

    /// exit the application
    Quit,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
enum VaultMode {
    Fido,
    Totp,
    Password,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
struct SelectedEntry {
    key_name: xous_ipc::String::<256>,
    description: xous_ipc::String::<256>,
    mode: VaultMode,
}

struct ItemLists {
    pub fido: BTreeMap::<String, ListItem>,
    pub totp: BTreeMap::<String, ListItem>,
    pub pw: BTreeMap::<String, ListItem>,
}
impl ItemLists {
    pub fn new() -> Self {
        ItemLists { fido: BTreeMap::new(), totp: BTreeMap::new(), pw: BTreeMap::new() }
    }
}

static SELF_CONN: AtomicU32 = AtomicU32::new(0);
const ERR_TIMEOUT_MS: usize = 5000;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let sid = xous::create_server().unwrap();
    // TODO: fix this so there is a uniform public API for the time server
    let time_conn = xous::connect(xous::SID::from_bytes(b"timeserverpublic").unwrap()).unwrap();
    let conn = xous::connect(sid).unwrap();

    start_fido_ux_thread(conn);

    // global shared state between threads.
    let mode = Arc::new(Mutex::new(VaultMode::Fido));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    let allow_host = Arc::new(AtomicBool::new(false));

    // spawn the actions server. This is responsible for grooming the UX elements. It
    // has to be in its own thread because it uses blocking modal calls that would cause
    // redraws of the background list to block/fail.
    let actions_sid = xous::create_server().unwrap();
    SELF_CONN.store(conn, Ordering::SeqCst);
    start_actions_thread(
        conn,
        actions_sid,
        mode.clone(),
        item_lists.clone(),
        action_active.clone()
    );
    let actions_conn = xous::connect(actions_sid).unwrap();


    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        let allow_host = allow_host.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let boot_time = ClockValue::new(tt.elapsed_ms() as i64, 1000);

            let mut vendor_session = VendorSession::default();

            let mut rng = ctap_crypto::rng256::XousRng256::new(&xns);
            // this call will block until the PDDB is mounted.
            let usb = usb_device_xous::UsbHid::new();
            // only run the main loop if the SoC is compatible
            if usb.is_soc_compatible() {
                let mut ctap_state = CtapState::new(&mut rng, check_user_presence, boot_time);
                let mut ctap_hid = CtapHid::new();
                loop {
                    match usb.u2f_wait_incoming() {
                        Ok(msg) => {
                            log::trace!("FIDO listener got message: {:?}", msg);
                            let now = ClockValue::new(tt.elapsed_ms() as i64, 1000);
                            let reply = match ctap_hid.process_hid_packet(&msg.packet, now, &mut ctap_state) {
                                ctap::hid::send::CTAPHIDResponse::StandardCommand(iter) => iter,
                                ctap::hid::send::CTAPHIDResponse::VendorCommand(cmd, cid, payload) => {
                                    match vendor_commands::handle_vendor_data(cmd, cid, payload, &mut vendor_session) {
                                        Ok(return_payload) => {
                                            // if None, this means we've finished parsing all that
                                            // was needed, and we handle/respond with real data

                                            match return_payload {
                                                Some(data) => data,
                                                None => {
                                                    log::debug!("starting processing of vendor data...");
                                                    let resp = vendor_commands::handle_vendor_command(
                                                        &mut vendor_session,
                                                        allow_host.load(Ordering::SeqCst)
                                                    );
                                                    log::debug!("finished processing of vendor data!");

                                                    match vendor_session.is_backup() {
                                                        true => {
                                                            if vendor_session.has_backup_data() {
                                                                resp
                                                            } else {
                                                                vendor_session = VendorSession::default();
                                                                resp
                                                            }
                                                        },
                                                        false => {
                                                            vendor_session = VendorSession::default();
                                                            resp
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(session_error) => {
                                            // reset the session
                                            vendor_session = VendorSession::default();

                                            session_error.ctaphid_error(cid)
                                        }
                                    }
                                    //vendor_commands::handle_vendor_command(cmd, cid, payload)
                                }
                            };
                            // This block handles sending packets.
                            for pkt_reply in reply {
                                let mut reply = RawFidoMsg::default();
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
                            match e {
                                xous::Error::ProcessTerminated => { // unplug happened, reset the authenticator
                                    log::info!("CTAP unplug_reset");
                                    ctap_state.unplug_reset();
                                },
                                _ => {
                                    log::warn!("FIDO listener got an error: {:?}", e);
                                }
                            }
                        }
                    }
                }
            } else {
                log::warn!("SoC rev is incompatible with USB HID operations, the U2F/FIDO2 server is not started");
            }
        }
    });

    // spawn the icontray handler
    let _ = thread::spawn({
        move || {
            icontray_server(conn);
        }
    });
    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(mode.clone(), pump_sid, conn, allow_host.clone());
    let pump_conn = xous::connect(pump_sid).unwrap();

    let menu_sid = xous::create_server().unwrap();
    let menu_mgr = submenu::create_submenu(conn, actions_conn, menu_sid);

    // this will block all initialization until the prereqs are met
    let (token, mut allow_redraw) = prereqs::prereqs(sid, time_conn);
    let mut vaultux = VaultUx::new(
        token,
        &xns,
        sid,
        menu_mgr,
        actions_conn,
        mode.clone(),
        item_lists,
        action_active.clone()
    );
    // Trigger the mode update in the actions
    send_message(actions_conn,
        Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
    ).ok();
    vaultux.update_mode();
    vaultux.get_glyph_style();

    // starts a thread to keep NTP up-to-date
    ntp_updater(time_conn);

    let modals = modals::Modals::new(&xns).unwrap();
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    loop {
        let msg = xous::receive_message(sid).unwrap();
        let opcode: Option<VaultOp> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(VaultOp::IncrementalLine) => {
                if action_active.load(Ordering::SeqCst) {
                    log::trace!("action active, skipping incremental input");
                    send_message(conn,
                        Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                    ).ok();
                    continue;
                }
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::debug!("Incremental input: {}", s.as_str());
                vaultux.input(s.as_str()).expect("Vault couldn't accept input string");
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
            }
            Some(VaultOp::Line) => {
                if action_active.load(Ordering::SeqCst) {
                    log::trace!("action active, skipping line input");
                    send_message(conn,
                        Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                    ).ok();
                    continue;
                }
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let s = buffer.as_flat::<xous_ipc::String<4000>, _>().unwrap();
                log::debug!("vaultux got input line: {}", s.as_str());
                match s.as_str() {
                    "\u{0011}" => {
                        *mode.lock().unwrap() = VaultMode::Fido;
                        send_message(actions_conn,
                            Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                        ).ok();
                        vaultux.update_mode();
                    }
                    "\u{0012}" => {
                        *mode.lock().unwrap() = VaultMode::Totp;
                        send_message(actions_conn,
                            Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                        ).ok();
                        vaultux.update_mode();
                        // this will start a periodic pump to keep the UX updating
                        send_message(pump_conn,
                            Message::new_scalar(totp::PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0)
                        ).ok();
                    }
                    "\u{0013}" => {
                        *mode.lock().unwrap() = VaultMode::Password;
                        send_message(actions_conn,
                            Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                        ).ok();
                        vaultux.update_mode();
                    }
                    "\u{0014}" => {
                        vaultux.raise_menu();
                    }
                    "↓" => {
                        vaultux.nav(NavDir::Down);
                    }
                    "↑" => {
                        vaultux.nav(NavDir::Up);
                    }
                    "←" => {
                        vaultux.nav(NavDir::PageUp);
                    }
                    "→" => {
                        vaultux.nav(NavDir::PageDown);
                    }
                    _ => {
                        // someone hit enter. The string is the whole search query, but what we care is that someone hit enter.
                        vaultux.raise_menu();
                    }
                }
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
            }
            Some(VaultOp::Redraw) => {
                if allow_redraw {
                    vaultux.redraw().expect("Vault couldn't redraw");
                }
            }
            Some(VaultOp::FullRedraw) => {
                vaultux.update_mode();
                if allow_redraw {
                    vaultux.redraw().expect("Vault couldn't redraw");
                }
            }
            Some(VaultOp::ReloadDbAndFullRedraw) => {
                send_message(actions_conn,
                    Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
                vaultux.update_mode();
                if allow_redraw {
                    vaultux.redraw().expect("Vault couldn't redraw");
                }
            }
            Some(VaultOp::BasisChange) => {
                vaultux.basis_change();
                // this set of calls will effectively force a reload of any UX data
                *mode.lock().unwrap() = VaultMode::Fido;
                send_message(actions_conn,
                    Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
                vaultux.update_mode();
                vaultux.input("").unwrap();
                send_message(conn,
                    Message::new_scalar(VaultOp::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
                ).ok();
            }
            Some(VaultOp::ChangeFocus) => xous::msg_scalar_unpack!(msg, new_state_code, _, _, _, {
                let new_state = gam::FocusState::convert_focus_change(new_state_code);
                vaultux.change_focus_to(&new_state);
                log::debug!("change focus: {:?}", new_state);
                match new_state {
                    gam::FocusState::Background => {
                        allow_redraw = false;
                    }
                    gam::FocusState::Foreground => {
                        // HID is always selected if the vault is foregrounded
                        vaultux.ensure_hid();
                        allow_redraw = true;
                    }
                }
            }),
            Some(VaultOp::MenuChangeFont) => {
                for item in FONT_LIST {
                    modals
                        .add_list_item(item)
                        .expect("couldn't build radio item list");
                }
                match modals.get_radiobutton(t!("vault.select_font", xous::LANG)) {
                    Ok(style) => {
                        vaultux.set_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                    },
                    _ => log::error!("get_radiobutton failed"),
                }
                vaultux.update_mode();
            }
            Some(VaultOp::MenuAutotype) => {
                modals.dynamic_notification(Some(t!("vault.autotyping", xous::LANG)), None).ok();
                match vaultux.autotype() {
                    Err(xous::Error::UseBeforeInit) => { // USB not plugged in
                        modals.dynamic_notification_update(Some(t!("vault.error.usb_error", xous::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::InvalidString) => { // deserialzation error
                        modals.dynamic_notification_update(Some(t!("vault.error.record_error", xous::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::ProcessNotFound) => { // key or dictionary not found
                        modals.dynamic_notification_update(Some(t!("vault.error.not_found", xous::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::InvalidPID) => { // nothing was selected
                        modals.dynamic_notification_update(Some(t!("vault.error.nothing_selected", xous::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::OutOfMemory) => { // trouble updating the key
                        modals.dynamic_notification_update(Some(t!("vault.error.update_error", xous::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    }
                    Ok(_) => {},
                    Err(e) => { // unknown error
                        modals.dynamic_notification(Some(
                            &format!("{}\n{:?}", t!("vault.error.internal_error", xous::LANG), e),
                        ), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    }
                }
                modals.dynamic_notification_close().ok();
                // force the one entry to update its UX cache so that the autotype time increments up
                if let Some(entry) = vaultux.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.send(actions_conn, ActionOp::UpdateOneItem.to_u32().unwrap()).expect("messaging error");
                }
            }
            Some(VaultOp::MenuDeleteStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX section.
                if let Some(entry) = vaultux.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.send(actions_conn, ActionOp::MenuDeleteStage2.to_u32().unwrap()).expect("messaging error");
                } else {
                    // this will block redraws, but it's just one notification in a sequence so it's OK.
                    modals.show_notification(t!("vault.error.nothing_selected", xous::LANG), None).ok();
                }
            }
            Some(VaultOp::MenuEditStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX section.
                if let Some(entry) = vaultux.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.send(actions_conn, ActionOp::MenuEditStage2.to_u32().unwrap()).expect("messaging error");
                } else {
                    // this will block redraws, but it's just one notification in a sequence so it's OK.
                    modals.show_notification(t!("vault.error.nothing_selected", xous::LANG), None).ok();
                }
            }
            Some(VaultOp::MenuReadoutMode) => {
                modals.dynamic_notification(Some(t!("vault.readout_switchover", xous::LANG)), None).ok();
                vaultux.readout_mode(true);
                modals.dynamic_notification_close().ok();

                allow_host.store(true, Ordering::SeqCst);
                modals.show_notification(t!("vault.readout_active", xous::LANG), None).ok();
                allow_host.store(false, Ordering::SeqCst);

                modals.dynamic_notification(Some(t!("vault.readout_switchover", xous::LANG)), None).ok();
                vaultux.readout_mode(false);
                modals.dynamic_notification_close().ok();
            }
            Some(VaultOp::Quit) => {
                log::error!("got Quit");
                break;
            }
            Some(VaultOp::Nop) => {},
            _ => {
                log::trace!("got unknown message {:?}", msg);
            }
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

pub(crate) fn basis_change() {
    log::info!("got basis change");
    xous::send_message(SELF_CONN.load(Ordering::SeqCst),
        Message::new_scalar(VaultOp::BasisChange.to_usize().unwrap(), 0, 0, 0, 0)
    ).unwrap();
}
