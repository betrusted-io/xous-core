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
mod itemcache;
use itemcache::*;

use locales::t;

use vault::ctap::main_hid::HidIterType;
use vault::env::xous::XousEnv;
use vault::env::Env;
use vault::{
    SELF_CONN, Transport, VaultOp
};

use actions::ActionOp;
use crate::ux::framework::NavDir;
use crate::prereqs::ntp_updater;
use crate::vendor_commands::VendorSession;

use ux::framework::{VaultUx, DEFAULT_FONT, FONT_LIST, name_to_style};
use xous_ipc::Buffer;
use xous::{send_message, Message, msg_blocking_scalar_unpack};
use usbd_human_interface_device::device::fido::*;
use num_traits::*;

use std::thread;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

// CTAP2 testing notes:
//
// Set a PIN for a token from the command line: `fido2-token -S /dev/hidraw0`
//
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
    pub key_guid: xous_ipc::String::<256>,
    pub description: xous_ipc::String::<256>,
    pub mode: VaultMode,
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
    // storage for lefty mode
    let lefty_mode = Arc::new(AtomicBool::new(false));

    // spawn the actions server. This is responsible for grooming the UX elements. It
    // has to be in its own thread because it uses blocking modal calls that would cause
    // redraws of the background list to block/fail.
    let actions_sid = xous::create_server().unwrap();
    SELF_CONN.store(conn, Ordering::SeqCst);
    let _ = thread::spawn({
        let main_conn = conn.clone();
        let sid = actions_sid.clone();
        let mode = mode.clone();
        let item_lists = item_lists.clone();
        let action_active = action_active.clone();
        let opensk_mutex = opensk_mutex.clone();
        move || {
            let mut manager = crate::actions::ActionManager::new(main_conn, mode, item_lists, action_active, opensk_mutex);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew(); // this is responsible for updating the item cache
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuDeleteStage2) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_delete(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuEditStage2) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_edit(entry); // this is responsible for updating the item cache
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the underlying UX
                        manager.activate();
                        manager.deactivate();
                    },
                    Some(ActionOp::UpdateOneItem) => {
                        let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.update_db_entry(entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _,{
                        // the password DBs are now not shared between modes, so no need to re-retrieve it.
                        if manager.is_db_empty() {
                            manager.retrieve_db();
                        }
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::ReloadDb) => msg_blocking_scalar_unpack!(msg, _, _, _, _,{
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                    #[cfg(feature="vault-testing")]
                    Some(ActionOp::GenerateTests) => {
                        manager.populate_tests();
                        manager.retrieve_db();
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });

    let actions_conn = xous::connect(actions_sid).unwrap();

    // spawn the FIDO USB->UX update kicker thread. It is responsible for issuing a UX refresh command
    // a few moments after FIDO traffic ceases. The purpose is to get the UX to reflect credential manipulaitons that happen via USB.
    let kicker_sid = xous::create_server().unwrap();
    let kicker_cid = xous::connect(kicker_sid).unwrap();
    // 64-bit elapsed time has to be split into two atomic U32s, because there is no atomic U64 on a 32-bit system :-/
    let kick_target_msb = Arc::new(AtomicU32::new(0));
    let kick_target_lsb = Arc::new(AtomicU32::new(0));
    let kicker_running = Arc::new(AtomicBool::new(false));
    const KICKER_DELAY_MS: u64 = 1500;
    let _ = thread::spawn({
        let main_conn = conn.clone();
        let self_conn = kicker_cid.clone();
        let kicker_running = kicker_running.clone();
        let kick_target_msb = kick_target_msb.clone();
        let kick_target_lsb = kick_target_lsb.clone();
        let mode = mode.clone();
        move || {
            let mut _msg_opt = None;
            let mut _return_type = 0;
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            loop {
                xous::reply_and_receive_next_legacy(kicker_sid, &mut _msg_opt, &mut _return_type)
                .unwrap();
                let now = tt.elapsed_ms();
                let target = ((kick_target_msb.load(Ordering::SeqCst) as u64) << 32)
                    | (kick_target_lsb.load(Ordering::SeqCst)) as u64;
                if now > target {
                    kicker_running.store(false, Ordering::SeqCst);
                    if *mode.lock().unwrap() == VaultMode::Fido {
                        log::debug!("kick!");
                        xous::send_message(main_conn,
                            xous::Message::new_scalar(VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(), 0, 0, 0, 0)
                        ).ok();
                    }
                } else {
                    // keep the poll alive until we exhaust our target time
                    tt.sleep_ms(KICKER_DELAY_MS as usize / 2).ok();
                    xous::send_message(self_conn,
                        xous::Message::new_scalar(0, 0, 0, 0, 0) // only one message type, any message will wake us up
                    ).ok();
                }
            }
        }
    });

    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        let allow_host = allow_host.clone();
        let opensk_mutex = opensk_mutex.clone();
        let conn = conn.clone();
        let lefty_mode = lefty_mode.clone();
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let mut vendor_session = VendorSession::default();
            let tt = ticktimer_server::Ticktimer::new().unwrap();
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
                        &format!("{}\n{:?}", t!("vault.migration_error", locales::LANG), e), None
                    ).ok();
                }
            };

            let env = XousEnv::new(conn, lefty_mode); // lefty_mode is now owned by env
            // only run the main loop if the SoC is compatible
            if env.is_soc_compatible() {
                let mut ctap = vault::Ctap::new(env, Instant::now());
                loop {
                    match ctap.env().main_hid_connection().u2f_wait_incoming() {
                        Ok(msg) => {
                            ctap.update_timeouts(Instant::now());
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
                    // note the traffic to a UX kicker. After KICKER_DELAY_MS of no USB activity, a redraw will be issued.
                    let target = tt.elapsed_ms() + KICKER_DELAY_MS;
                    if !kicker_running.swap(true, Ordering::SeqCst) {
                        // update the MSB first, so in case of rollover the only negative effect is we wait an extra KICKER_DELAY_MS
                        kick_target_msb.store((target >> 32) as u32, Ordering::SeqCst);
                        kick_target_lsb.store(target as u32, Ordering::SeqCst);
                        // kick the kicker!
                        xous::send_message(kicker_cid,
                            xous::Message::new_scalar(0, 0, 0, 0, 0) // only one message type, any message will do
                        ).ok();
                    } else {
                        kick_target_msb.store((target >> 32) as u32, Ordering::SeqCst);
                        kick_target_lsb.store(target as u32, Ordering::SeqCst);
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
        item_lists.clone(),
        action_active.clone()
    );
    vaultux.update_mode();
    vaultux.get_glyph_style();

    // starts a thread to keep NTP up-to-date
    ntp_updater(time_conn);

    // gets the user preferences that configure vault
    let prefs = userprefs::Manager::new();
    let mut autotype_delay_ms = prefs.autotype_rate_or_value(30).unwrap();
    vaultux.set_autotype_delay_ms(autotype_delay_ms);
    lefty_mode.store(prefs.lefty_mode_or_value(false).unwrap(), Ordering::SeqCst);

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
                    Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0)
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
                    Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0)
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
                match modals.get_radiobutton(t!("vault.select_font", locales::LANG)) {
                    Ok(style) => {
                        vaultux.set_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                    },
                    _ => log::error!("get_radiobutton failed"),
                }
                vaultux.update_mode();
            }
            Some(VaultOp::MenuAutotype) => {
                modals.dynamic_notification(Some(t!("vault.autotyping", locales::LANG)), None).ok();
                match vaultux.autotype() {
                    Err(xous::Error::UseBeforeInit) => { // USB not plugged in
                        modals.dynamic_notification_update(Some(t!("vault.error.usb_error", locales::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::InvalidString) => { // deserialzation error
                        modals.dynamic_notification_update(Some(t!("vault.error.record_error", locales::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::ProcessNotFound) => { // key or dictionary not found
                        modals.dynamic_notification_update(Some(t!("vault.error.not_found", locales::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::InvalidPID) => { // nothing was selected
                        modals.dynamic_notification_update(Some(t!("vault.error.nothing_selected", locales::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    },
                    Err(xous::Error::OutOfMemory) => { // trouble updating the key
                        modals.dynamic_notification_update(Some(t!("vault.error.update_error", locales::LANG)), None).ok();
                        tt.sleep_ms(ERR_TIMEOUT_MS).unwrap();
                    }
                    Ok(_) => {},
                    Err(e) => { // unknown error
                        modals.dynamic_notification(Some(
                            &format!("{}\n{:?}", t!("vault.error.internal_error", locales::LANG), e),
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
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                }
            }
            Some(VaultOp::MenuEditStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX section.
                log::debug!("selecting entry for edit");
                if let Some(entry) = vaultux.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.send(actions_conn, ActionOp::MenuEditStage2.to_u32().unwrap()).expect("messaging error");
                } else {
                    // this will block redraws, but it's just one notification in a sequence so it's OK.
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                }
            }
            Some(VaultOp::MenuReadoutMode) => {
                modals.dynamic_notification(Some(t!("vault.readout_switchover", locales::LANG)), None).ok();
                vaultux.readout_mode(true);
                modals.dynamic_notification_close().ok();

                allow_host.store(true, Ordering::SeqCst);
                modals.show_notification(t!("vault.readout_active", locales::LANG), None).ok();
                allow_host.store(false, Ordering::SeqCst);

                modals.dynamic_notification(Some(t!("vault.readout_switchover", locales::LANG)), None).ok();
                vaultux.readout_mode(false);
                modals.dynamic_notification_close().ok();
            }
            Some(VaultOp::MenuAutotypeRate) => {
                let cv = {
                    let mut rate = prefs.autotype_rate_or_default().unwrap();
                    if rate == 0 {
                        rate = 30;
                    }
                    rate
                };
                let raw = modals
                    .alert_builder(t!("prefs.autotype_rate_in_ms", locales::LANG))
                    .field(
                        Some(cv.to_string()),
                        Some(|tf| match tf.as_str().parse::<usize>() {
                            Ok(_) => None,
                            Err(_) => Some(xous_ipc::String::from_str(
                                t!("prefs.autobacklight_err", locales::LANG),
                            )),
                        }),
                    )
                    .build()
                    .unwrap();
                autotype_delay_ms = raw.first().as_str().parse::<usize>().unwrap(); // we know this is a number, we checked with validator;
                prefs.set_autotype_rate(autotype_delay_ms).unwrap();
                vaultux.set_autotype_delay_ms(autotype_delay_ms);
            }
            Some(VaultOp::MenuLeftyMode) => {
                let cv = prefs.lefty_mode_or_default().unwrap();

                modals.add_list(vec![t!("prefs.yes", locales::LANG), t!("prefs.no", locales::LANG)]).unwrap();
                let mode = yes_no_to_bool(
                    modals
                        .get_radiobutton(&format!("{} {}", t!("prefs.current_setting", locales::LANG),
                            bool_to_yes_no(cv)))
                        .unwrap()
                        .as_str(),
                );
                prefs.set_lefty_mode(mode).unwrap();
                lefty_mode.store(mode, Ordering::SeqCst);
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

fn bool_to_yes_no(val: bool) -> String {
    match val {
        true => t!("prefs.yes", locales::LANG).to_owned(),
        false => t!("prefs.no", locales::LANG).to_owned(),
    }
}
fn yes_no_to_bool(val: &str) -> bool {
    if val == t!("prefs.yes", locales::LANG) {
        true
    } else if val == t!("prefs.no", locales::LANG) {
        false
    } else {
        unreachable!("cannot go here!");
    }
}