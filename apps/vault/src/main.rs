#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod ux;
mod submenu;
mod actions;
mod totp;
mod prereqs;
mod vendor_commands;
mod storage;
mod migration_v1;

use locales::t;

use vault::ctap::main_hid::HidIterType;
use vault::env::xous::XousEnv;
use vault::env::Env;
use vault::{
    SELF_CONN, Transport, VaultOp
};

use actions::{ActionOp, start_actions_thread};
use crate::ux::framework::{ListItem, NavDir};
use crate::prereqs::ntp_updater;
use crate::vendor_commands::VendorSession;

use ux::framework::{VaultUx, DEFAULT_FONT, FONT_LIST, name_to_style};
use xous_ipc::Buffer;
use xous::{send_message, Message};
use usbd_human_interface_device::device::fido::*;
use num_traits::*;

use std::thread;
use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::collections::BTreeMap;


// CTAP2 testing notes:
// run our branch and use this to forward the prompts on to the device:
// netcat -k -u -l 6502 > /dev/ttyS0
// use the "autotest" feature to remove some excess prompts that interfere with the test

// the OpenSK code is based off of commit 0db393bd1e9c3901f772b3c2107dbbeb71ff2dc5 from the
// Google OpenSK repository. This commit is dated Jan 4 2023. The initial merge into Xous
// was finished on Jan 27 2023. Any patches to this code base will have to be manually
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
    - FIDO2   (U2F authenticators)
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

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Fido,
    Totp,
    Password,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct SelectedEntry {
    pub key_name: xous_ipc::String::<256>,
    pub description: xous_ipc::String::<256>,
    pub mode: VaultMode,
}
pub struct ItemLists {
    pub fido: BTreeMap::<String, ListItem>,
    pub totp: BTreeMap::<String, ListItem>,
    pub pw: BTreeMap::<String, ListItem>,
}
impl ItemLists {
    pub fn new() -> Self {
        ItemLists { fido: BTreeMap::new(), totp: BTreeMap::new(), pw: BTreeMap::new() }
    }
}

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

    // global shared state between threads.
    let mode = Arc::new(Mutex::new(VaultMode::Fido));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    let allow_host = Arc::new(AtomicBool::new(false));
    // Protects access to the openSK PDDB entries from simultaneous readout on the UX while OpenSK is updating it
    let opensk_mutex = Arc::new(Mutex::new(0));

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
        action_active.clone(),
        opensk_mutex.clone(),
    );
    let actions_conn = xous::connect(actions_sid).unwrap();


    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        let allow_host = allow_host.clone();
        let opensk_mutex = opensk_mutex.clone();
        let conn = conn.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let mut vendor_session = VendorSession::default();
            // block until the PDDB is mounted
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();
            // Attempt a migration, if necessary. If none is necessary this call is fairly lightweight.
            match crate::migration_v1::migrate(&pddb) {
                Ok(_) => {},
                Err(e) => {
                    log::warn!("Migration encountered errors! {:?}", e);
                    let modals = modals::Modals::new(&xns).unwrap();
                    modals.show_notification(
                        &format!("{}\n{:?}", t!("vault.migration_error", xous::LANG), e), None
                    ).ok();
                }
            };

            let env = XousEnv::new(conn);
            // only run the main loop if the SoC is compatible
            if env.is_soc_compatible() {
                let mut ctap = vault::Ctap::new(env, Instant::now());
                loop {
                    match ctap.env().main_hid_connection().u2f_wait_incoming() {
                        Ok(msg) => {
                            let mutex = opensk_mutex.lock().unwrap();
                            log::trace!("Received U2F packet");
                            let typed_reply = ctap.process_hid_packet(&msg.packet, Transport::MainHid, Instant::now());
                            match typed_reply {
                                HidIterType::Ctap(reply) => {
                                    for pkt_reply in reply {
                                        let mut reply = RawFidoMsg::default();
                                        reply.packet.copy_from_slice(&pkt_reply);
                                        let status = ctap.env().main_hid_connection().u2f_send(reply);
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
                                HidIterType::Vendor(msg) => {
                                    let reply =
                                    match vendor_commands::handle_vendor_data(
                                        msg.cmd as u8,
                                        msg.cid,
                                        msg.payload,
                                        &mut vendor_session
                                    ) {
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

                                            session_error.ctaphid_error(msg.cid)
                                        }
                                    };
                                    for pkt_reply in reply {
                                        let mut reply = RawFidoMsg::default();
                                        reply.packet.copy_from_slice(&pkt_reply);
                                        let status = ctap.env().main_hid_connection().u2f_send(reply);
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
                            }
                            drop(mutex);
                        }
                        Err(e) => {
                            match e {
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
            crate::ux::icontray::icontray_server(conn);
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
    vaultux.update_mode();
    vaultux.get_glyph_style();

    // starts a thread to keep NTP up-to-date
    ntp_updater(time_conn);

    let modals = modals::Modals::new(&xns).unwrap();
    let tt = ticktimer_server::Ticktimer::new().unwrap();
    let mut first_time = true;
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
                        if first_time {
                            // Populate the initial fields, just the first time
                            send_message(actions_conn,
                                Message::new_blocking_scalar(ActionOp::UpdateMode.to_usize().unwrap(), 0, 0, 0, 0)
                            ).ok();
                            vaultux.update_mode();
                            first_time = false;
                        }
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
