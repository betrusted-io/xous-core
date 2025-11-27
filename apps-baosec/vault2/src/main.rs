mod ux;
use ux::*;
mod itemcache;
use itemcache::*;
mod actions;
use actions::ActionOp;
mod storage;
mod submenu;
mod totp;
pub mod vault_api;
pub use vault_api::*;
mod vendor_commands;

use core::sync::atomic::{AtomicBool, Ordering};
use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use locales::t;
use num_traits::*;
use pddb::Pddb;
use totp::PumpOp;
use vault2::Transport;
use vault2::ctap::main_hid::HidIterType;
use vault2::env::Env;
use vault2::env::xous::XousEnv;
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;
use xous_usb_hid::device::fido::*;

use crate::vendor_commands::VendorSession;

/*
Dev status & notes --

Nov 12 2025:

`cargo xtask baosec-emu` will launch the emulated UI.

  -[x] implement RTC
  -[x] implement QR code scanning loop
  -[x] implement HID loop - needs testing, but have to implement RTC first before can test on HW
  -[x] implement "action" loop

  -[ ] figure out interaction for searching for passwords
    -[ ] search by QR code
    -[ ] search by text - is this even a good idea? i think the UX is not great, so maybe we omit it entirely.
-[ ] implement PIN management UI - entering a pin unlocks the deniable basis automatically
    -[ ] remove basis management calls when replacing with PIN calls

-[ ] Consider the side channel resistance of the AES implementation in the keystore. We may want
    to use an AES API that explicitly wraps the SCE's masked AES implementation to reduce side channels,
    versus the Vex CPU core's AES implementation

-[ ] deniable basis implementation
    -[ ] The PIN may be optionally set to one of three values:
    -[ ] nothing (implemented as all 0's in the cryptographic key)
    -[ ] a passcode (4-8 digits entered via keyboard)
      -[ ] Configurable self-destruct after # of incorrect PIN guesses. Set at 8 by default.
      -[ ] Each incorrect guess has a 15 second timeout
    -[ ] a QR code (generated using browser & managed by user, 128 bits entropy)
  -[ ] Remember that re-pinning should be a low-cost operation. As such the PIN is used
   to protect a wrapped key that actually encrypts the PDDB, and not be the key itself.
  -[ ] One deniable basis at a time is allowed. It is mounted using using only:
    -[ ] a PIN (4-8 digits entered via keyboard)
    -[ ] a QR code (generated using browser & managed by user)

-[ ] implement backups
  -[ ] PIN confirmation required to set system into backup mode
  -[ ] Backup key is displayed. This is 256-bit key, displayed as QR code or Bip-39.
  -[ ] Hash & integrity blocks computed, stored
  -[ ] Backup data presented as .bin file on mass storage device
  -[ ] Device stays in backup state until user clears screen; exit via reboot


UI interaction planning.

Main mode of interaction is QR code scanning. This should be accessible with a single button. Thus:

1. middle center button pops up QR code scanner. Behavior then depends on the code scanned.
  Note: will need a menu item to replace passwords - we should keep the old passwords in case it's needed?

Observation: left/right paging buttons don't do a lot with O(hundreds) passwords, but scrolling
is fast. So don't implement left/right paging as on Precursor, freeing up two buttons.

2. Left button: pops up text entry to filter lists

3. Right button: "action" button - used to type the current password, and/or approve FIDO sigs

4. Up/down/select jog: exclusively for menu interactions. Menus are always linear, with select.

This UI design does not allow for hierarchical menus because there isn't a "back" button, but
we *could*, possibly, if we really needed menu hierarchies, repurpose a left/right button as
a hierarchy nav function.

-> But can we keep the menu shallow?
  */

pub(crate) const SERVER_NAME_VAULT2: &str = "_Vault2_";

#[derive(Copy, Clone, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VaultMode {
    Totp,
    Password,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct SelectedEntry {
    pub key_guid: String,
    pub description: String,
    pub mode: VaultMode,
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("Vault2 PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let tt = ticktimer_server::Ticktimer::new().unwrap();

    // Register the server with xous
    let sid = xns.register_name(SERVER_NAME_VAULT2, None).expect("can't register server");
    let conn = xous::connect(sid).unwrap();

    // global shared state
    let mode = Arc::new(Mutex::new(VaultMode::Totp));
    let allow_totp_rendering = Arc::new(AtomicBool::new(true));
    let item_lists = Arc::new(Mutex::new(ItemLists::new()));
    let action_active = Arc::new(AtomicBool::new(false));
    // Protects access to the openSK PDDB entries from simultaneous readout on the UX while OpenSK is updating
    let opensk_mutex = Arc::new(Mutex::new(0));
    let allow_host = Arc::new(AtomicBool::new(false));
    // storage for lefty mode
    let lefty_mode = Arc::new(AtomicBool::new(false));

    let mut vault_ui = VaultUi::new(&xns, conn, item_lists.clone(), mode.clone());

    // spawn the TOTP pumper
    let pump_sid = xous::create_server().unwrap();
    crate::totp::pumper(mode.clone(), pump_sid, conn, allow_totp_rendering.clone());
    let pump_conn = xous::connect(pump_sid).unwrap();

    // respond to keyboard events
    let kbd = bao1x_api::keyboard::Keyboard::new(&xns).unwrap();
    kbd.register_listener(SERVER_NAME_VAULT2, VaultOp::KeyPress.to_u32().unwrap() as usize);

    // spawn the actions server. This is responsible for grooming the UX elements. It
    // has to be in its own thread because it uses blocking modal calls that would cause
    // redraws of the background list to block/fail.
    let actions_sid = xous::create_server().unwrap();

    let _ = thread::spawn({
        let main_conn = conn.clone();
        let sid = actions_sid.clone();
        let mode = mode.clone();
        let item_lists = item_lists.clone();
        let action_active = action_active.clone();
        move || {
            let mut manager = crate::actions::ActionManager::new(main_conn, mode, item_lists, action_active);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew(); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuDeleteStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_delete(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuEditStage2) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.menu_edit(entry); // this is responsible for updating the item cache
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().clear(VaultMode::Password); // clear the cached item list for passwords
                        manager.retrieve_db();
                        manager.deactivate();
                    }
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the
                        // underlying UX
                        manager.activate();
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateOneItem) => {
                        let buffer =
                            unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                        let entry = buffer.to_original::<SelectedEntry, _>().unwrap();
                        manager.activate();
                        manager.update_db_entry(entry);
                        manager.deactivate();
                    }
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        // the password DBs are now not shared between modes, so no need to re-retrieve it.
                        if manager.is_db_empty() {
                            manager.retrieve_db();
                        }
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::ReloadDb) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::AcquireQr) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                        manager.acquire_qr();
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                    #[cfg(feature = "vault-testing")]
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

    // spawn the FIDO2 USB handler
    let _ = thread::spawn({
        let allow_host = allow_host.clone();
        let opensk_mutex = opensk_mutex.clone();
        let conn = conn.clone();
        let lefty_mode = lefty_mode.clone();
        move || {
            let mut vendor_session = VendorSession::default();
            // block until the PDDB is mounted
            let pddb = pddb::Pddb::new();
            pddb.is_mounted_blocking();

            let env = XousEnv::new(conn, lefty_mode); // lefty_mode is now owned by env
            let mut ctap = vault2::Ctap::new(env, Instant::now());
            loop {
                match ctap.env().main_hid_connection().u2f_wait_incoming() {
                    Ok(msg) => {
                        ctap.update_timeouts(Instant::now());
                        let mutex = opensk_mutex.lock().unwrap();
                        log::trace!("Received U2F packet");
                        let typed_reply =
                            ctap.process_hid_packet(&msg.packet, Transport::MainHid, Instant::now());
                        match typed_reply {
                            HidIterType::Ctap(reply) => {
                                for pkt_reply in reply {
                                    let mut reply = RawFidoReport::default();
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
                                let reply = match vendor_commands::handle_vendor_data(
                                    msg.cmd as u8,
                                    msg.cid,
                                    msg.payload,
                                    &mut vendor_session,
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
                                                    allow_host.load(Ordering::SeqCst),
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
                                                    }
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
                                    let mut reply = RawFidoReport::default();
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
                    Err(e) => match e {
                        _ => {
                            log::warn!("FIDO listener got an error: {:?}", e);
                        }
                    },
                }
            }
        }
    });

    let menu_sid = xous::create_server().unwrap();
    let menu_mgr = submenu::create_submenu(conn, actions_conn, menu_sid);
    let modals = modals::Modals::new(&xns).unwrap();
    vault_ui.apply_glyph_style();

    // give the system a second to stabilize, then try to mount
    tt.sleep_ms(1000).ok();
    let pddb = pddb::Pddb::new();
    pddb.try_mount();

    // reload the database
    xous::send_message(
        actions_conn,
        xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .ok();
    vault_ui.refresh_totp();

    // kickstart the pumper
    xous::send_message(pump_conn, xous::Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0))
        .expect("couldn't start the pumper");
    let mut menu_active = false;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        log::trace!("Got message: {:?}", msg.body.id());
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(VaultOp::Redraw) => {
                vault_ui.redraw();
            }
            Some(VaultOp::ReloadDbAndFullRedraw) => {
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                vault_ui.refresh_totp();
                vault_ui.redraw();
            }
            Some(VaultOp::MenuDone) => {
                menu_active = false;
                // update the TOTP codes, in case there were changes
                vault_ui.refresh_totp();
                allow_totp_rendering.store(true, Ordering::SeqCst);
                vault_ui.redraw();
            }
            Some(VaultOp::MenuTotpMode) => {
                *mode.lock().unwrap() = VaultMode::Totp;
                // reload DB on mode switch
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                vault_ui.refresh_totp();
                allow_totp_rendering.store(true, Ordering::SeqCst);
                xous::send_message(
                    pump_conn,
                    xous::Message::new_scalar(PumpOp::Pump.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .expect("couldn't start the pumper");
                vault_ui.redraw();
            }
            Some(VaultOp::MenuPwMode) => {
                *mode.lock().unwrap() = VaultMode::Password;
                allow_totp_rendering.store(false, Ordering::SeqCst);
                // reload DB on mode switch
                xous::send_message(
                    actions_conn,
                    xous::Message::new_blocking_scalar(ActionOp::ReloadDb.to_usize().unwrap(), 0, 0, 0, 0),
                )
                .ok();
                vault_ui.redraw();
            }
            Some(VaultOp::KeyPress) => xous::msg_scalar_unpack!(msg, k1, _k2, _k3, _k4, {
                let k = char::from_u32(k1 as u32).unwrap_or('\u{0000}');
                log::info!("key {}", k);
                if menu_active {
                    menu_mgr.key_press(k);
                } else {
                    match k {
                        '∴' => {
                            allow_totp_rendering.store(false, Ordering::SeqCst);
                            menu_mgr.redraw();
                            menu_active = true;
                        }
                        '↓' => {
                            vault_ui.nav(NavDir::Down);
                            vault_ui.redraw();
                        }
                        '↑' => {
                            vault_ui.nav(NavDir::Up);
                            vault_ui.redraw();
                        }
                        '←' => {
                            vault_ui.nav(NavDir::PageUp);
                            vault_ui.redraw();
                        }
                        '→' => {
                            vault_ui.nav(NavDir::PageDown);
                            vault_ui.redraw();
                        }
                        '🔥' => {
                            allow_totp_rendering.store(false, Ordering::SeqCst);
                            xous::send_message(
                                actions_conn,
                                xous::Message::new_blocking_scalar(
                                    ActionOp::AcquireQr.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .ok();
                            // wait a moment for the last frame to clear before redrawing the UI
                            tt.sleep_ms(100).ok();
                            allow_totp_rendering.store(true, Ordering::SeqCst);
                            // reload DB to pickup the new data
                            xous::send_message(
                                actions_conn,
                                xous::Message::new_blocking_scalar(
                                    ActionOp::ReloadDb.to_usize().unwrap(),
                                    0,
                                    0,
                                    0,
                                    0,
                                ),
                            )
                            .ok();
                            if *mode.lock().unwrap() == VaultMode::Totp {
                                vault_ui.refresh_totp();
                            }
                            vault_ui.redraw();
                        }
                        _ => {
                            log::trace!("unhandled key {}", k);
                        }
                    }
                }
            }),
            Some(VaultOp::MenuEditStage1) => {
                // stage 1 happens here because the filtered list and selection entry are in the responsive UX
                // section.
                log::debug!("selecting entry for edit");
                if let Some(entry) = vault_ui.selected_entry() {
                    let buf = Buffer::into_buf(entry).expect("IPC error");
                    buf.send(actions_conn, ActionOp::MenuEditStage2.to_u32().unwrap())
                        .expect("messaging error");
                } else {
                    // this will block redraws
                    allow_totp_rendering.store(false, Ordering::SeqCst);
                    modals.show_notification(t!("vault.error.nothing_selected", locales::LANG), None).ok();
                    allow_totp_rendering.store(true, Ordering::SeqCst);
                }
            }
            Some(VaultOp::MenuChangeFont) => {
                for item in FONT_LIST {
                    modals.add_list_item(item).expect("couldn't build radio item list");
                }
                allow_totp_rendering.store(false, Ordering::SeqCst);
                match modals.get_radiobutton(t!("vault.select_font", locales::LANG)) {
                    Ok(style) => {
                        vault_ui.store_glyph_style(name_to_style(&style).unwrap_or(DEFAULT_FONT));
                        vault_ui.apply_glyph_style();
                    }
                    _ => log::error!("get_radiobutton failed"),
                }
                allow_totp_rendering.store(true, Ordering::SeqCst);
            }
            Some(VaultOp::BasisChange) => {
                vault_ui.basis_change();
                xous::send_message(
                    conn,
                    xous::Message::new_blocking_scalar(
                        VaultOp::ReloadDbAndFullRedraw.to_usize().unwrap(),
                        0,
                        0,
                        0,
                        0,
                    ),
                )
                .ok();
            }
            _ => {
                log::error!("Got unknown message: {:?}", msg);
            }
        }
    }
}
