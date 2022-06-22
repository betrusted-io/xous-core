use std::thread;
use gam::TextEntryPayload;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use num_traits::*;
use xous::{SID, msg_blocking_scalar_unpack, Message, send_message};
use xous_ipc::Buffer;
use locales::t;
use std::io::{Write, Read};
use passwords::PasswordGenerator;
use chrono::{Utc, DateTime, NaiveDateTime};
use std::time::{SystemTime, UNIX_EPOCH};
use std::cell::RefCell;

use crate::ux::ListItem;
use crate::{VaultMode, SelectedEntry};

pub(crate) const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
pub(crate) const VAULT_ALLOC_HINT: usize = 256;
const VAULT_PASSWORD_REC_VERSION: u32 = 1;
/// time allowed between dialog box swaps for background operations to redraw
const SWAP_DELAY_MS: usize = 300;

pub(crate) struct PasswordRecord {
    pub version: u32,
    pub description: String,
    pub username: String,
    pub password: String,
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ActionOp {
    /// Menu items
    MenuAddnew,
    MenuEditStage2,
    MenuDeleteStage2,
    MenuClose,
    /// Internal ops
    UpdateMode,
    Quit,
}

pub(crate) fn start_actions_thread(
    main_conn: xous::CID,
    sid: SID, mode: Arc::<Mutex::<VaultMode>>,
    item_list: Arc::<Mutex::<Vec::<ListItem>>>,
    action_active: Arc::<AtomicBool>,
) {
    let _ = thread::spawn({
        move || {
            let mut manager = ActionManager::new(main_conn, mode, item_list, action_active);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew();
                        // this is necessary so the next redraw shows the newly added entry
                        manager.retrieve_db();
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
                        manager.menu_edit(entry);
                        manager.retrieve_db();
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuClose) => {
                        // dummy activate/de-activate cycle because we have to trigger a redraw of the underlying UX
                        manager.activate();
                        manager.deactivate();
                    },
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _,{
                        manager.retrieve_db();
                        xous::return_scalar(msg.sender, 1).unwrap();
                    }),
                    Some(ActionOp::Quit) => {
                        break;
                    }
                    None => {
                        log::error!("msg could not be decoded {:?}", msg);
                    }
                }
            }
            xous::destroy_server(sid).ok();
        }
    });
}

struct ActionManager {
    modals: modals::Modals,
    trng: RefCell::<trng::Trng>,
    mode: Arc::<Mutex::<VaultMode>>,
    item_list: Arc::<Mutex::<Vec::<ListItem>>>,
    pddb: RefCell::<pddb::Pddb>,
    tt: ticktimer_server::Ticktimer,
    action_active: Arc::<AtomicBool>,
    mode_cache: VaultMode,
    main_conn: xous::CID,
}
impl ActionManager {
    pub fn new(
        main_conn: xous::CID,
        mode: Arc::<Mutex::<VaultMode>>,
        item_list: Arc::<Mutex::<Vec::<ListItem>>>,
        action_active: Arc::<AtomicBool>
    ) -> ActionManager {
        let xns = xous_names::XousNames::new().unwrap();
        let mc = (*mode.lock().unwrap()).clone();
        ActionManager {
            modals: modals::Modals::new(&xns).unwrap(),
            trng: RefCell::new(trng::Trng::new(&xns).unwrap()),
            mode_cache: mc,
            mode,
            item_list,
            pddb: RefCell::new(pddb::Pddb::new()),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            action_active,
            main_conn,
        }
    }
    pub(crate) fn activate(&mut self) {
        // there's a "two phase" lock -- we indicate we're "active" with this here AtomicBool
        // the drawing thread promises not to change the mode of the UI when this is true
        // in return, we get to grab a copy of the operating mode variable, which allows the
        // drawing thread to proceed as it relies also on reading this shared state to draw its UI.
        self.mode_cache = {
            (*self.mode.lock().unwrap()).clone()
        };
        self.action_active.store(true, Ordering::SeqCst);
        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap(); // allow calling menu to close
    }
    pub(crate) fn deactivate(&self) {
        self.action_active.store(false, Ordering::SeqCst);
        send_message(self.main_conn,
            Message::new_scalar(
                crate::VaultOp::FullRedraw.to_usize().unwrap(),
                0, 0, 0, 0
            )
        ).ok();
    }
    pub(crate) fn menu_addnew(&mut self) {
        match self.mode_cache {
            VaultMode::Password => {
                let description = match self.modals
                    .alert_builder(t!("vault.newitem.name", xous::LANG))
                    .field(None, Some(name_validator))
                    .build()
                {
                    Ok(text) => {
                        text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
                    },
                    _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let username = match self.modals
                    .alert_builder(t!("vault.newitem.username", xous::LANG))
                    .field(None, Some(name_validator))
                    .build()
                {
                    Ok(text) => text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string(),
                    _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let mut approved = false;
                // Security note about PasswordGenerator. This is a 3rd party crate. It relies on `rand`'s implementation
                // of ThreadRng to generate passwords. As of the version committed to the lockfile, I have evidenced the
                // ThreadRng to request 8 bytes of entropy from our TRNG to seed its state. If the docs are to be trusted,
                // its thread-local RNG is a ChaCha CSPRNG, although the number of rounds used in it is not clear; code says
                // 12 rounds, code comments say 20 and reference an issue about how this should be reduced.
                // Audit path
                // Cargo.lock is at:
                //  rand-0.8.5
                //  rand_core 0.6.3
                //  getrandom 0.2.6 -> xous fork via Patch in top level Cargo.toml to map crates-io.getrandom to imports/getrandom
                //  rand_chacha 0.3.1
                //  passwords 3.1.9
                //  random-pick 1.2.15
                //  random-number 0.1.7
                //  random-number-macro-mipl 0.1.6
                //  proc-macro-hack : 0.5.19...and more (syn/quote also pulled in)
                // - PasswordGenerator
                //   - PasswordGeneratorIter::generate()
                //   - random_pick::pick_multiple_from_multiple_slices()
                //     - random_pick::gen_multiple_usize_with_weights()
                //       - rng = random_number::rand::thread_rng()
                //         - ThreadRng::thread_rng()
                //         - Some crazy unsafe refcell construction that returns a clone of a
                //           ReseedingRng<Core, OsRng>
                //           - rand-0.8.5::std line 13: pub(crate) use rand_chacha::ChaCha12Core as Core;
                //           - confirm no feature flags gating this, it is always used
                //           - OsRng::try_fill_bytes()
                //             - getrandom() -> to Xous code
                //               - getrandom Xous fork - imp::getrandom_inner()
                //                 - getrandom Xous fork - ensure_trng_conn() then fill_bytes() native Xous call
                //       - random_number::random!(0..high, rng)
                //         - random_number::random_with_rng
                //           - random_number::random_inclusively_with_rng()
                //             - Uniform::new_inclusive().sample()
                //               - dead end at Distribution Trait and UniformSampler Trait, let's hope this is correct?
                let pg = PasswordGenerator {
                    length: 20,
                    numbers: true,
                    lowercase_letters: true,
                    uppercase_letters: true,
                    symbols: true,
                    spaces: false,
                    exclude_similar_characters: true,
                    strict: true,
                };
                let mut password = pg.generate_one().unwrap();
                while !approved {
                    let maybe_password = match self.modals
                        .alert_builder(t!("vault.newitem.password", xous::LANG))
                        .field(Some(password), Some(name_validator))
                        .build()
                    {
                        Ok(text) => {
                            text.content()[0].content.as_str().unwrap().to_string()
                        },
                        _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                    };
                    self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                    password = if maybe_password.len() == 0 {
                        let length = match self.modals
                            .alert_builder(t!("vault.newitem.configure_length", xous::LANG))
                            .field(Some("20".to_string()), Some(length_validator))
                            .build()
                        {
                            Ok(entry) => entry.content()[0].content.as_str().unwrap().parse::<u32>().unwrap(),
                            _ => {log::error!("Length entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                        };
                        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                        let mut upper = false;
                        let mut number = false;
                        let mut symbol = false;
                        self.modals
                            .add_list(vec![
                                t!("vault.newitem.uppercase", xous::LANG),
                                t!("vault.newitem.numbers", xous::LANG),
                                t!("vault.newitem.symbols", xous::LANG),
                            ]).expect("couldn't create configuration modal");
                        match self.modals.get_checkbox(t!("vault.newitem.configure_generator", xous::LANG)) {
                            Ok(options) => {
                                for opt in options {
                                    if opt == t!("vault.newitem.uppercase", xous::LANG) {upper = true;}
                                    if opt == t!("vault.newitem.numbers", xous::LANG) {number = true;}
                                    if opt == t!("vault.newitem.symbols", xous::LANG) {symbol = true;}
                                }
                            }
                            _ => {log::error!("Modal selection error"); self.action_active.store(false, Ordering::SeqCst); return}
                        }
                        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                        let pg2 = PasswordGenerator {
                            length: length as usize,
                            numbers: number,
                            lowercase_letters: true,
                            uppercase_letters: upper,
                            symbols: symbol,
                            spaces: false,
                            exclude_similar_characters: true,
                            strict: true,
                        };
                        approved = false;
                        pg2.generate_one().unwrap()
                    } else {
                        approved = true;
                        maybe_password
                    };
                }
                let record = PasswordRecord {
                    version: VAULT_PASSWORD_REC_VERSION,
                    description,
                    username,
                    password,
                    notes: t!("vault.notes", xous::LANG).to_string(),
                    ctime: utc_now().timestamp() as u64,
                    atime: 0,
                    count: 0,
                };
                let ser = serialize_password(&record);
                let guid = self.gen_guid();
                log::info!("storing into guid: {}", guid);
                match self.pddb.borrow().get(
                    VAULT_PASSWORD_DICT,
                    &guid,
                    None, true, true,
                    Some(VAULT_ALLOC_HINT), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        match data.write(&ser) {
                            Ok(_) => {}
                            Err(e) => {
                                self.modals.show_notification(&format!("{}\n{:?}",
                                    t!("vault.error.internal_error", xous::LANG), e
                                ), None).ok();
                            }
                        }
                    }
                    Err(e) => {
                        self.modals.show_notification(&format!("{}\n{:?}",
                            t!("vault.error.internal_error", xous::LANG), e
                        ), None).ok();
                    }
                }
                self.pddb.borrow().sync().ok();
            }
            _ => {} // not valid for these other modes
        }
    }

    pub(crate) fn menu_delete(&mut self, entry: SelectedEntry) {
        if self.yes_no_approval(&format!("{}\n{}", t!("vault.delete.confirm", xous::LANG), entry.description)) {
            let dict = match entry.mode {
                VaultMode::Password => VAULT_PASSWORD_DICT,
                VaultMode::Fido => crate::fido::U2F_APP_DICT,
                VaultMode::Totp => unimplemented!(),
            };
            match self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap_or("UTF8-error"), None) {
                Ok(_) => {
                    self.modals.show_notification(t!("vault.completed", xous::LANG), None).ok();
                }
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::NotFound => {
                            // handle special case of FIDO which is two dicts combined
                            if entry.mode == VaultMode::Fido {
                                // try the "other" dictionary
                                match self.pddb.borrow()
                                .delete_key(
                                    crate::ctap::FIDO_CRED_DICT,
                                    entry.key_name.as_str().unwrap_or("UTF8-error"),
                                    None) {
                                        Ok(_) => {
                                            self.modals.show_notification(t!("vault.completed", xous::LANG), None).ok();
                                            return;
                                        }
                                        _ => {}
                                }
                            }
                            self.modals.show_notification(t!("vault.error.not_found", xous::LANG), None).ok();
                        }
                        _ => {
                            self.modals.show_notification(&format!("{}\n{:?}",
                                t!("vault.error.internal_error", xous::LANG),
                                e
                            ), None).ok();
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn menu_edit(&mut self, entry: SelectedEntry) {
        let dict = match entry.mode {
            VaultMode::Password => VAULT_PASSWORD_DICT,
            VaultMode::Fido => crate::fido::U2F_APP_DICT,
            VaultMode::Totp => unimplemented!(),
        };
        match entry.mode {
            VaultMode::Password => {
                let maybe_update = match self.pddb.borrow().get(
                    dict, entry.key_name.as_str().unwrap(), None,
                    false, false, None, Some(crate::basis_change)
                ) {
                    Ok(mut record) => {
                        let mut data = Vec::<u8>::new();
                        let maybe_update = match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut pw) = deserialize_password(data) {
                                    let edit_data = self.modals
                                        .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                        .field(Some(pw.description), Some(name_validator))
                                        .field(Some(pw.username), Some(name_validator))
                                        .field(Some(pw.password), Some(name_validator))
                                        .field(Some(pw.notes), Some(name_validator))
                                        .build().expect("modals error in edit");
                                    pw.description = edit_data.content()[0].content.as_str().unwrap().to_string();
                                    pw.username = edit_data.content()[1].content.as_str().unwrap().to_string();
                                    pw.password = edit_data.content()[2].content.as_str().unwrap().to_string();
                                    pw.notes = edit_data.content()[3].content.as_str().unwrap().to_string();
                                    pw.atime = utc_now().timestamp() as u64;
                                    Some(pw)
                                } else {
                                    log::error!("couldn't deserialize {}", entry.key_name);
                                    self.modals.show_notification(t!("vault.error.record_error", xous::LANG), None).ok();
                                    None
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't access key {}: {:?}", entry.key_name, e);
                                self.modals.show_notification(&format!("{}\n{:?}",
                                    t!("vault.error.internal_error", xous::LANG), e),
                                    None
                                ).ok();
                                None
                            }
                        };
                        maybe_update
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                self.modals.show_notification(t!("vault.error.not_found", xous::LANG), None).ok();
                            }
                            _ => {
                                self.modals.show_notification(&format!("{}\n{:?}",
                                    t!("vault.error.internal_error", xous::LANG),
                                    e
                                ), None).ok();
                            }
                        }
                        None
                    }
                };
                if let Some(update) = maybe_update {
                    match self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap(), None) {
                        Ok(_) => {}
                        Err(e) => {
                            self.modals.show_notification(&format!("{}\n{:?}",
                                t!("vault.error.internal_error", xous::LANG),
                                e
                            ), None).ok();
                            return;
                        }
                    }
                    match self.pddb.borrow().get(
                        dict, entry.key_name.as_str().unwrap(), None,
                        false, true, Some(VAULT_ALLOC_HINT),
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let ser = serialize_password(&update);
                            match record.write(&ser) {
                                Ok(_) => {}
                                Err(e) => {
                                    self.modals.show_notification(&format!("{}\n{:?}",
                                        t!("vault.error.internal_error", xous::LANG), e
                                    ), None).ok();
                                }
                            }
                        }
                        Err(e) => {
                            self.modals.show_notification(&format!("{}\n{:?}",
                                t!("vault.error.internal_error", xous::LANG), e
                            ), None).ok();
                            return;
                        }
                    }
                }
                self.pddb.borrow().sync().ok();
            }
            VaultMode::Fido => {
                // at the moment only U2F records are supported for editing. The FIDO2 stuff is done with a different record
                // storage format that's a bit funkier to edit.
                let maybe_update = match self.pddb.borrow().get(
                    dict, entry.key_name.as_str().unwrap(), None,
                    false, false, None, Some(crate::basis_change)
                ) {
                    Ok(mut record) => {
                        let mut data = Vec::<u8>::new();
                        let maybe_update = match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut ai) = crate::fido::deserialize_app_info(data) {
                                    let edit_data = self.modals
                                        .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                        .field(Some(ai.name), Some(name_validator))
                                        .field(Some(ai.notes), Some(name_validator))
                                        .field(Some(hex::encode(ai.id)), None)
                                        .build().expect("modals error in edit");
                                    ai.name = edit_data.content()[0].content.as_str().unwrap().to_string();
                                    ai.notes = edit_data.content()[1].content.as_str().unwrap().to_string();
                                    ai.atime = utc_now().timestamp() as u64;
                                    Some(ai)
                                } else {
                                    log::error!("couldn't deserialize {}", entry.key_name);
                                    self.modals.show_notification(t!("vault.error.record_error", xous::LANG), None).ok();
                                    None
                                }
                            }
                            Err(e) => {
                                log::error!("couldn't access key {}: {:?}", entry.key_name, e);
                                self.modals.show_notification(&format!("{}\n{:?}",
                                    t!("vault.error.internal_error", xous::LANG), e),
                                    None
                                ).ok();
                                None
                            }
                        };
                        maybe_update
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                // most likely this is due to this being a FIDO2 token, which we can't edit
                                // there are no editable fields -- if we change them, it can break the authentication protocol.
                                self.modals.show_notification(t!("vault.error.fido2", xous::LANG), None).ok();
                            }
                            _ => {
                                self.modals.show_notification(&format!("{}\n{:?}",
                                    t!("vault.error.internal_error", xous::LANG),
                                    e
                                ), None).ok();
                            }
                        }
                        None
                    }
                };
                if let Some(update) = maybe_update {
                    match self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap(), None) {
                        Ok(_) => {}
                        Err(e) => {
                            self.modals.show_notification(&format!("{}\n{:?}",
                                t!("vault.error.internal_error", xous::LANG),
                                e
                            ), None).ok();
                            return;
                        }
                    }
                    match self.pddb.borrow().get(
                        dict, entry.key_name.as_str().unwrap(), None,
                        false, true, Some(VAULT_ALLOC_HINT),
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let ser = crate::fido::serialize_app_info(&update);
                            match record.write(&ser) {
                                Ok(_) => {}
                                Err(e) => {
                                    self.modals.show_notification(&format!("{}\n{:?}",
                                        t!("vault.error.internal_error", xous::LANG), e
                                    ), None).ok();
                                }
                            }
                        }
                        Err(e) => {
                            self.modals.show_notification(&format!("{}\n{:?}",
                                t!("vault.error.internal_error", xous::LANG), e
                            ), None).ok();
                            return;
                        }
                    }
                }
                self.pddb.borrow().sync().ok();
            }
            VaultMode::Totp => {
                unimplemented!()
            }
        }
    }

    fn yes_no_approval(&self, query: &str) -> bool {
        self.modals.add_list(
            vec![t!("vault.yes", xous::LANG), t!("vault.no", xous::LANG)]
        ).expect("couldn't build confirmation dialog");
        match self.modals.get_radiobutton(query) {
            Ok(response) => {
                if &response == t!("vault.yes", xous::LANG) {
                    true
                } else {
                    false
                }
            }
            _ => {
                log::error!("get approval failed");
                false
            },
        }
    }

    fn gen_guid(&self) -> String {
        let mut guid = [0u8; 16];
        self.trng.borrow_mut().fill_bytes(&mut guid);
        hex::encode(guid)
    }

    /// Populate the display list with data from the PDDB. Limited by total available RAM; probably
    /// would stop working if you have over 500-1k records with the current heap limits.
    pub(crate) fn retrieve_db(&mut self) {
        self.mode_cache = {
            (*self.mode.lock().unwrap()).clone()
        };
        let il = &mut *self.item_list.lock().unwrap();
        il.clear();
        match self.mode_cache {
            VaultMode::Password => {
                let keylist = match self.pddb.borrow().list_keys(VAULT_PASSWORD_DICT, None) {
                    Ok(keylist) => keylist,
                    Err(e) => {
                        log::error!("error accessing password database: {:?}", e);
                        Vec::new()
                    }
                };
                for key in keylist {
                    match self.pddb.borrow().get(
                        VAULT_PASSWORD_DICT,
                        &key,
                        None,
                        false, false, None,
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let mut data = Vec::<u8>::new();
                            match record.read_to_end(&mut data) {
                                Ok(_len) => {
                                    if let Some(pw) = deserialize_password(data) {
                                        let extra = format!("{}; {}{}",
                                            crate::ux::atime_to_str(pw.atime),
                                            t!("vault.u2f.appinfo.authcount", xous::LANG),
                                            pw.count,
                                        );
                                        let desc = format!("{} @ {}", pw.username, pw.description);
                                        let li = ListItem {
                                            name: desc,
                                            extra,
                                            dirty: true,
                                            guid: key,
                                        };
                                        il.push(li);
                                    } else {
                                        log::error!("couldn't deserialize {}", key);
                                    }
                                }
                                Err(e) => log::error!("couldn't access key {}: {:?}", key, e),
                            }
                        }
                        Err(e) => log::error!("couldn't access key {}: {:?}", key, e),
                    }
                }
            }
            VaultMode::Fido => {
                il.push(ListItem { name: "test.com".to_string(), extra: "Used 5 mins ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "google.com".to_string(), extra: "Never used".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "my app".to_string(), extra: "Used 2 hours ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "💎🙌".to_string(), extra: "Used 2 days ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "百度".to_string(), extra: "Used 1 month ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "duplicate.com".to_string(), extra: "Used 1 week ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "duplicate.com".to_string(), extra: "Used 8 mins ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "amawhat.com".to_string(), extra: "Used 6 days ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "amazon.com".to_string(), extra: "Used 3 days ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "amazingcode.org".to_string(), extra: "Never used".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "ziggyziggyziggylongdomain.com".to_string(), extra: "Never used".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "another long domain name.com".to_string(), extra: "Used 2 months ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "bunniestudios.com".to_string(), extra: "Used 30 mins ago".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "github.com".to_string(), extra: "Used 6 hours ago".to_string(), dirty: true, guid: self.gen_guid() });
            }
            VaultMode::Totp => {
                il.push(ListItem { name: "gmail.com".to_string(), extra: "162 321".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "google.com".to_string(), extra: "445 768".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "my 图片 app".to_string(), extra: "982 111".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "🍕🍔🍟🌭".to_string(), extra: "056 182".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "百度".to_string(), extra: "111 111".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "duplicate.com".to_string(), extra: "462 124".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "duplicate.com".to_string(), extra: "462 124".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "amazon.com".to_string(), extra: "842 012".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "ziggyziggyziggylongdomain.com".to_string(), extra: "462 212".to_string(), dirty: true, guid: self.gen_guid() });
                il.push(ListItem { name: "github.com".to_string(), extra: "Used 6 hours ago".to_string(), dirty: true, guid: self.gen_guid() });
            }
        }
        il.sort();
    }
}


pub(crate) fn name_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let proposed_name = input.as_str();
    if proposed_name.contains('\n') { // the '\n' is reserved as the delimiter to end the name field
        Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_char", xous::LANG)))
    } else {
        None
    }
}
fn length_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let text_str = input.as_str();
    match text_str.parse::<u32>() {
        Ok(input_int) => if input_int < 1 || input_int > 128 {
            Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_number", xous::LANG)))
        } else {
            None
        },
        _ => Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_number", xous::LANG))),
    }
}

pub(crate) fn serialize_password<'a>(record: &PasswordRecord) -> Vec::<u8> {
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}",
        "version", record.version,
        "description", record.description,
        "username", record.username,
        "password", record.password,
        "notes", record.notes,
        "ctime", record.ctime,
        "atime", record.atime,
        "count", record.count,
    ).into_bytes()
}

pub(crate) fn deserialize_password(data: Vec::<u8>) -> Option<PasswordRecord> {
    if let Ok(desc_str) = String::from_utf8(data) {
        let mut pr = PasswordRecord {
            version: 0,
            description: String::new(),
            username: String::new(),
            password: String::new(),
            notes: String::new(),
            ctime: 0,
            atime: 0,
            count: 0
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            return None;
                        }
                    }
                    "description" => pr.description.push_str(data),
                    "username" => pr.username.push_str(data),
                    "password" => pr.password.push_str(data),
                    "notes" => pr.notes.push_str(data),
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            return None;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            pr.atime = atime;
                        } else {
                            return None;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            pr.count = count;
                        } else {
                            return None;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing app info, aborting", tag);
                        return None;
                    }
                }
            }
        }
        Some(pr)
    } else {
        None
    }
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid target
fn utc_now() -> DateTime::<Utc> {
    let now =
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}
