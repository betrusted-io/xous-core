use core::convert::TryFrom;
use std::{thread, io::ErrorKind};
use gam::TextEntryPayload;
use pddb::BasisRetentionPolicy;
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

use crate::{ux::{ListItem, deserialize_app_info}, ctap::FIDO_CRED_DICT};
use crate::{VaultMode, SelectedEntry};

use crate::fido::U2F_APP_DICT;
use crate::totp::TotpAlgorithm;

pub(crate) const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
pub(crate) const VAULT_TOTP_DICT: &'static str = "vault.totp";
/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
pub(crate) const VAULT_ALLOC_HINT: usize = 256;
pub(crate) const VAULT_TOTP_ALLOC_HINT: usize = 128;
const VAULT_PASSWORD_REC_VERSION: u32 = 1;
const VAULT_TOTP_REC_VERSION: u32 = 1;
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

pub(crate) struct TotpRecord {
    pub version: u32,
    // as base32, RFC4648 no padding
    pub secret: String,
    pub name: String,
    pub algorithm: TotpAlgorithm,
    pub notes: String,
    pub digits: u32,
    pub timestep: u64,
    pub ctime: u64,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ActionOp {
    /// Menu items
    MenuAddnew,
    MenuEditStage2,
    MenuDeleteStage2,
    MenuClose,
    MenuUnlockBasis,
    MenuManageBasis,
    /// Internal ops
    UpdateMode,
    Quit,
    #[cfg(feature="testing")]
    /// Testing
    GenerateTests,
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
                    Some(ActionOp::MenuUnlockBasis) => {
                        manager.activate();
                        manager.unlock_basis();
                        manager.retrieve_db();
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.retrieve_db();
                        manager.deactivate();
                    }
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
                    #[cfg(feature="testing")]
                    Some(ActionOp::GenerateTests) => {
                        manager.populate_tests();
                        manager.retrieve_db();
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
                    .field(None, Some(password_validator))
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
                    .field(None, Some(password_validator))
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
                        .field(Some(password), Some(password_validator))
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
                log::debug!("storing into guid: {}", guid);
                match self.pddb.borrow().get(
                    VAULT_PASSWORD_DICT,
                    &guid,
                    None, true, true,
                    Some(VAULT_ALLOC_HINT), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        match data.write(&ser) {
                            Ok(len) => log::debug!("wrote {} bytes", len),
                            Err(e) => {log::error!("internal error");
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))},
                        }
                    }
                    Err(e) => { log::error!("internal error");
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))},
                }
                log::debug!("syncing...");
                self.pddb.borrow().sync().ok();
            }
            VaultMode::Fido => {
                self.report_err(t!("vault.error.add_fido2", xous::LANG), None::<std::io::Error>);
            }
            VaultMode::Totp => {
                let description = match self.modals
                    .alert_builder(t!("vault.newitem.name", xous::LANG))
                    .field(None, Some(password_validator))
                    .build()
                {
                    Ok(text) => {
                        text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
                    },
                    _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let secret = match self.modals
                    .alert_builder(t!("vault.newitem.totp_ss", xous::LANG))
                    .field(None, Some(totp_ss_validator))
                    .build()
                {
                    Ok(text) => {
                        text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
                    },
                    _ => {log::error!("TOTP ss entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let ss = secret.to_uppercase();
                let ss_vec = if let Some(ss) = base32::decode(base32::Alphabet::RFC4648 { padding: false }, &ss) {
                    ss
                } else {
                    if let Some(ss) = base32::decode(base32::Alphabet::RFC4648 { padding: true }, &ss) {
                        ss
                    } else {
                        if let Some(ss) = base32::decode(base32::Alphabet::Crockford, &ss) {
                            ss
                        } else {
                            log::error!("Shouldn't have happened: validated shared secret didn't decode!");
                            Vec::new()
                        }
                    }
                };
                let validated_secret = base32::encode(base32::Alphabet::RFC4648 { padding: false }, &ss_vec);
                // time, hash, etc. are all the "expected defaults" -- if you want to change them, edit the record after entering it.
                let totp = TotpRecord {
                    version: VAULT_TOTP_REC_VERSION,
                    name: description,
                    secret: validated_secret,
                    algorithm: TotpAlgorithm::HmacSha1,
                    digits: 6,
                    timestep: 30,
                    ctime: utc_now().timestamp() as u64,
                    notes: t!("vault.notes", xous::LANG).to_string(),
                };
                let ser = serialize_totp(&totp);
                let guid = self.gen_guid();
                log::debug!("storing into guid: {}", guid);
                match self.pddb.borrow().get(
                    VAULT_TOTP_DICT,
                    &guid,
                    None, true, true,
                    Some(VAULT_TOTP_ALLOC_HINT), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        match data.write(&ser) {
                            Ok(len) => log::debug!("wrote {} bytes", len),
                            Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                        }
                    }
                    Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                }
                log::debug!("syncing...");
                self.pddb.borrow().sync().ok();
            }
        }
    }

    pub(crate) fn menu_delete(&mut self, entry: SelectedEntry) {
        if self.yes_no_approval(&format!("{}\n{}", t!("vault.delete.confirm", xous::LANG), entry.description)) {
            let dict = match entry.mode {
                VaultMode::Password => VAULT_PASSWORD_DICT,
                VaultMode::Fido => crate::fido::U2F_APP_DICT,
                VaultMode::Totp => VAULT_TOTP_DICT,
            };
            // first "get" the key, to resolve exactly what basis the key is in. This is because `delete_key()` will
            // only look in the most recently unlocked secret basis, it won't automatically descend into the database
            // and try to cull something willy-nilly.
            match self.pddb.borrow().get(dict, entry.key_name.as_str().unwrap_or("UTF8-error"),
                None, false, false, None, None::<fn()>
            ) {
                Ok(candidate) => {
                    let attr = candidate.attributes().expect("couldn't get key attributes");
                    match self.pddb.borrow().delete_key(dict,
                        entry.key_name.as_str().unwrap_or("UTF8-error"),
                        Some(&attr.basis)) {
                        Ok(_) => {
                            self.modals.show_notification(t!("vault.completed", xous::LANG), None).ok();
                        }
                        Err(e) => {
                            self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e));
                        }
                    }
                }
                Err(e) => {
                    match e.kind() {
                        std::io::ErrorKind::NotFound => {
                            // handle special case of FIDO which is two dicts combined
                            if entry.mode == VaultMode::Fido {
                                // try the "other" dictionary
                                match self.pddb.borrow().get(crate::ctap::FIDO_CRED_DICT, entry.key_name.as_str().unwrap_or("UTF8-error"),
                                    None, false, false, None, None::<fn()>
                                ) {
                                    Ok(candidate) => {
                                        let attr = candidate.attributes().expect("couldn't get key attributes");
                                        match self.pddb.borrow()
                                        .delete_key(
                                            crate::ctap::FIDO_CRED_DICT,
                                            entry.key_name.as_str().unwrap_or("UTF8-error"),
                                            Some(&attr.basis)
                                        ) {
                                            Ok(_) => {
                                                self.modals.show_notification(t!("vault.completed", xous::LANG), None).ok();
                                            }
                                            Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                                        }
                                    }
                                    Err(e) => {
                                        self.report_err(t!("vault.error.not_found", xous::LANG), Some(e));
                                    }
                                }
                            } else {
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e));
                            }
                        }
                        _ => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                    }
                }
            }
        }
    }

    pub(crate) fn menu_edit(&mut self, entry: SelectedEntry) {
        let dict = match entry.mode {
            VaultMode::Password => VAULT_PASSWORD_DICT,
            VaultMode::Fido => crate::fido::U2F_APP_DICT,
            VaultMode::Totp => VAULT_TOTP_DICT,
        };
        match entry.mode {
            VaultMode::Password => {
                let maybe_update = match self.pddb.borrow().get(
                    dict, entry.key_name.as_str().unwrap(), None,
                    false, false, None, Some(crate::basis_change)
                ) {
                    Ok(mut record) => {
                        // resolve the basis of the key, so that we are editing it "in place"
                        let attr = record.attributes().expect("couldn't get key attributes");
                        let mut data = Vec::<u8>::new();
                        let maybe_update = match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut pw) = deserialize_password(data) {
                                    let edit_data = self.modals
                                        .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                        .field(Some(pw.description), Some(password_validator))
                                        .field(Some(pw.username), Some(password_validator))
                                        .field(Some(pw.password), Some(password_validator))
                                        .field(Some(pw.notes), Some(password_validator))
                                        .build().expect("modals error in edit");
                                    pw.description = edit_data.content()[0].content.as_str().unwrap().to_string();
                                    pw.username = edit_data.content()[1].content.as_str().unwrap().to_string();
                                    pw.password = edit_data.content()[2].content.as_str().unwrap().to_string();
                                    pw.notes = edit_data.content()[3].content.as_str().unwrap().to_string();
                                    pw.atime = utc_now().timestamp() as u64;
                                    pw
                                } else { log::error!("record error");
                                    self.report_err(t!("vault.error.record_error", xous::LANG), None::<std::io::Error>); return }
                            }
                            Err(e) => { log::error!("internal error"); self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); return }
                        };
                        Some((maybe_update, attr.basis))
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                log::error!("not found");
                                self.report_err(t!("vault.error.not_found", xous::LANG), None::<std::io::Error>)
                            },
                            _ => {
                                log::error!("internal error");
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))
                            },
                        }
                        None
                    }
                };
                if let Some((update, basis)) = maybe_update {
                    self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap(), Some(&basis))
                    .unwrap_or_else(|e| {log::error!("internal error");
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))});
                    match self.pddb.borrow().get(
                        dict, entry.key_name.as_str().unwrap(), Some(&basis),
                        false, true, Some(VAULT_ALLOC_HINT),
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let ser = serialize_password(&update);
                            record.write(&ser)
                            .unwrap_or_else(|e| {
                                log::error!("internal error");
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); 0});
                        }
                        Err(e) => {
                            log::error!("internal error");
                            self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))
                        },
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
                        // resolve the basis of the key, so that we are editing it "in place"
                        let attr = record.attributes().expect("couldn't get key attributes");
                        let mut data = Vec::<u8>::new();
                        let maybe_update = match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut ai) = crate::fido::deserialize_app_info(data) {
                                    let edit_data = self.modals
                                        .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                        .field(Some(ai.name), Some(password_validator))
                                        .field(Some(ai.notes), Some(password_validator))
                                        .field(Some(hex::encode(ai.id)), None)
                                        .build().expect("modals error in edit");
                                    ai.name = edit_data.content()[0].content.as_str().unwrap().to_string();
                                    ai.notes = edit_data.content()[1].content.as_str().unwrap().to_string();
                                    ai.atime = utc_now().timestamp() as u64;
                                    ai
                                } else { self.report_err(t!("vault.error.record_error", xous::LANG), None::<std::io::Error>); return }
                            }
                            Err(e) => { self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); return }
                        };
                        Some((maybe_update, attr.basis))
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => self.report_err(t!("vault.error.fido2", xous::LANG), None::<std::io::Error>),
                            _ => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                        }
                        return
                    }
                };
                if let Some((update, basis)) = maybe_update {
                    self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap(), Some(&basis))
                    .unwrap_or_else(|e| self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)));
                    match self.pddb.borrow().get(
                        dict, entry.key_name.as_str().unwrap(), Some(&basis),
                        false, true, Some(VAULT_ALLOC_HINT),
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let ser = crate::fido::serialize_app_info(&update);
                            record.write(&ser).unwrap_or_else(|e| {
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); 0});
                        }
                        Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                    }
                }
                self.pddb.borrow().sync().ok();
            }
            VaultMode::Totp => {
                let maybe_update = match self.pddb.borrow().get(
                    dict, entry.key_name.as_str().unwrap(), None,
                    false, false, None, Some(crate::basis_change)
                ) {
                    Ok(mut record) => {
                        // resolve the basis of the key, so that we are editing it "in place"
                        let attr = record.attributes().expect("couldn't get key attributes");
                        let mut data = Vec::<u8>::new();
                        let maybe_update = match record.read_to_end(&mut data) {
                            Ok(_len) => {
                                if let Some(mut pw) = deserialize_totp(data) {
                                    let alg: String = pw.algorithm.into();
                                    let edit_data = self.modals
                                        .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                        .field(Some(pw.name), Some(password_validator))
                                        .field(Some(pw.secret), Some(password_validator))
                                        .field(Some(pw.notes), Some(password_validator))
                                        .field(Some(pw.timestep.to_string()), Some(password_validator))
                                        .field(Some(alg), Some(password_validator))
                                        .field(Some(pw.digits.to_string()), Some(password_validator))
                                        .build().expect("modals error in edit");
                                    pw.name = edit_data.content()[0].content.as_str().unwrap().to_string();
                                    pw.secret = edit_data.content()[1].content.as_str().unwrap().to_string();
                                    pw.notes = edit_data.content()[2].content.as_str().unwrap().to_string();
                                    if let Ok(t) = u64::from_str_radix(edit_data.content()[3].content.as_str().unwrap(), 10) {
                                        pw.timestep = t;
                                    }
                                    if let Ok(alg) = TotpAlgorithm::try_from(edit_data.content()[4].content.as_str().unwrap()) {
                                        pw.algorithm = alg;
                                    }
                                    if let Ok(d) = u32::from_str_radix(edit_data.content()[5].content.as_str().unwrap(), 10) {
                                        pw.digits = d;
                                    }
                                    pw
                                } else { self.report_err(t!("vault.error.record_error", xous::LANG), None::<std::io::Error>); return }
                            }
                            Err(e) => { self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); return }
                        };
                        Some((maybe_update, attr.basis))
                    }
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => self.report_err(t!("vault.error.not_found", xous::LANG), None::<std::io::Error>),
                            _ => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                        }
                        return
                    }
                };
                if let Some((update, basis)) = maybe_update {
                    self.pddb.borrow().delete_key(dict, entry.key_name.as_str().unwrap(), Some(&basis))
                    .unwrap_or_else(|e| self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)));
                    match self.pddb.borrow().get(
                        dict, entry.key_name.as_str().unwrap(), Some(&basis),
                        false, true, Some(VAULT_ALLOC_HINT),
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let ser = serialize_totp(&update);
                            record.write(&ser).unwrap_or_else(|e| {
                                self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); 0});
                        }
                        Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                    }
                }
                self.pddb.borrow().sync().ok();
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
                let start = self.tt.elapsed_ms();
                let keylist = match self.pddb.borrow().list_keys(VAULT_PASSWORD_DICT, None) {
                    Ok(keylist) => keylist,
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                log::debug!("Password dictionary not yet created");
                            }
                            _ => {
                                log::error!("Dictionary error accessing password database");
                                self.report_err("Dictionary error accessing password database", Some(e))
                            },
                        }
                        Vec::new()
                    }
                };
                log::info!("listing took {} ms", self.tt.elapsed_ms() - start);
                let start = self.tt.elapsed_ms();
                let klen = keylist.len();
                for key in keylist {
                    match self.pddb.borrow().get(
                        VAULT_PASSWORD_DICT,
                        &key,
                        None,
                        false, false, None,
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            // determine the exact length of the record and read it in one go.
                            // read_to_end() performs ~5x read calls to do the same thing, because it
                            // has to "guess" the total record length starting with a 32-byte increment
                            let len = record.attributes().unwrap().len;
                            let mut data = Vec::<u8>::with_capacity(len);
                            data.resize(len, 0);
                            match record.read_exact(&mut data) {
                                Ok(_len) => {
                                    if let Some(pw) = deserialize_password(data) {
                                        let extra = format!("{}; {}{}",
                                            crate::ux::atime_to_str(pw.atime),
                                            t!("vault.u2f.appinfo.authcount", xous::LANG),
                                            pw.count,
                                        );
                                        let desc = format!("{}/{}", pw.description, pw.username);
                                        let li = ListItem {
                                            name: desc,
                                            extra,
                                            dirty: true,
                                            guid: key,
                                        };
                                        il.push(li);
                                    } else {
                                        log::error!("Couldn't deserialize password");
                                        self.report_err("Couldn't deserialize password:", Some(key));
                                    }
                                }
                                Err(e) => {
                                    log::error!("Couldn't access password key");
                                    self.report_err("Couldn't access password key", Some(e))
                                },
                            }
                        }
                        Err(e) => {
                            log::error!("Couldn't access password key");
                            self.report_err("Couldn't access password key", Some(e))
                        },
                    }
                }
                log::info!("readout took {} ms for {} elements", self.tt.elapsed_ms() - start, klen);
            }
            VaultMode::Fido => {
                // first assemble U2F records
                log::debug!("listing in {}", U2F_APP_DICT);
                let keylist = match self.pddb.borrow().list_keys(U2F_APP_DICT, None) {
                    Ok(keylist) => keylist,
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                log::debug!("U2F dictionary not yet created");
                            }
                            _ => self.report_err("Dictionary error accessing U2F database", Some(e)),
                        }
                        Vec::new()
                    }
                };
                log::debug!("list: {:?}", keylist);
                for key in keylist {
                    match self.pddb.borrow().get(
                        U2F_APP_DICT,
                        &key,
                        None,
                        false, false, None,
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let len = record.attributes().unwrap().len;
                            let mut data = Vec::<u8>::with_capacity(len);
                            data.resize(len, 0);
                            match record.read_exact(&mut data) {
                                Ok(_len) => {
                                    if let Some(ai) = deserialize_app_info(data) {
                                        let extra = format!("{}; {}{}",
                                            crate::ux::atime_to_str(ai.atime),
                                            t!("vault.u2f.appinfo.authcount", xous::LANG),
                                            ai.count,
                                        );
                                        let desc = format!("{}", ai.name);
                                        let li = ListItem {
                                            name: desc,
                                            extra,
                                            dirty: true,
                                            guid: key,
                                        };
                                        il.push(li);
                                    } else {
                                        self.report_err("Couldn't deserialize U2F:", Some(key));
                                    }
                                }
                                Err(e) => self.report_err("Couldn't access U2F key", Some(e)),
                            }
                        }
                        Err(e) => self.report_err("Couldn't access U2F key", Some(e)),
                    }
                }
                log::debug!("listing in {}", FIDO_CRED_DICT);
                let keylist = match self.pddb.borrow().list_keys(FIDO_CRED_DICT, None) {
                    Ok(keylist) => keylist,
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                log::debug!("FIDO2 dictionary not yet created");
                            }
                            _ => self.report_err("Dictionary error accessing FIDO2 database", Some(e)),
                        }
                        Vec::new()
                    }
                };
                log::debug!("keylist: {:?}", keylist);
                // now merge in the FIDO2 records
                for key in keylist {
                    match self.pddb.borrow().get(
                        FIDO_CRED_DICT,
                        &key,
                        None,
                        false, false, None,
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let len = record.attributes().unwrap().len;
                            let mut data = Vec::<u8>::with_capacity(len);
                            data.resize(len, 0);
                            match record.read_exact(&mut data) {
                                Ok(_len) => {
                                    match crate::ctap::storage::deserialize_credential(&data) {
                                        Some(result) => {
                                            let name = if let Some(display_name) = result.user_display_name {
                                                display_name
                                            } else {
                                                String::from_utf8(result.user_handle).unwrap_or("".to_string())
                                            };
                                            let desc = format!("{} / {}", result.rp_id, String::from_utf8(result.credential_id).unwrap_or("---".to_string()));
                                            let extra = format!("FIDO2 {}", name);
                                            let li = ListItem {
                                                name: desc,
                                                extra,
                                                dirty: true,
                                                guid: key,
                                            };
                                            il.push(li);
                                        }
                                        None => self.report_err("Couldn't deserialize FIDO2:", Some(key)),
                                    }
                                }
                                Err(e) => self.report_err("Couldn't access FIDO2 key", Some(e)),
                            }
                        }
                        Err(e) => self.report_err("Couldn't access FIDO2 key", Some(e)),
                    }
                }
            }
            VaultMode::Totp => {
                let keylist = match self.pddb.borrow().list_keys(VAULT_TOTP_DICT, None) {
                    Ok(keylist) => keylist,
                    Err(e) => {
                        match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                log::debug!("TOTP dictionary not yet created");
                            }
                            _ => self.report_err("Dictionary error accessing TOTP database", Some(e)),
                        }
                        Vec::new()
                    }
                };
                for key in keylist {
                    match self.pddb.borrow().get(
                        VAULT_TOTP_DICT,
                        &key,
                        None,
                        false, false, None,
                        Some(crate::basis_change)
                    ) {
                        Ok(mut record) => {
                            let len = record.attributes().unwrap().len;
                            let mut data = Vec::<u8>::with_capacity(len);
                            data.resize(len, 0);
                            match record.read_exact(&mut data) {
                                Ok(_len) => {
                                    if let Some(totp) = deserialize_totp(data) {
                                        let alg: String = totp.algorithm.into();
                                        let extra = format!("{}:{}:{}:{}", totp.secret, totp.digits, totp.timestep, alg);
                                        let desc = format!("{}", totp.name);
                                        let li = ListItem {
                                            name: desc,
                                            extra,
                                            dirty: true,
                                            guid: key,
                                        };
                                        il.push(li);
                                    } else {
                                        self.report_err("Couldn't deserialize TOTP:", Some(key));
                                    }
                                }
                                Err(e) => self.report_err("Couldn't access TOTP key", Some(e)),
                            }
                        }
                        Err(e) => self.report_err("Couldn't access TOTP key", Some(e)),
                    }
                }
            }
        }
        il.sort();
    }

    pub(crate) fn unlock_basis(&mut self) {
        let name = match self.modals
            .alert_builder(t!("vault.basis.name", xous::LANG))
            .field(None, Some(name_validator))
            .build()
        {
            Ok(text) => {
                text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
            },
            _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
        };
        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
        match self.pddb.borrow().unlock_basis(&name, Some(BasisRetentionPolicy::Persist)) {
            Ok(_) => log::debug!("Basis {} unlocked", name),
            Err(e) => match e.kind() {
                ErrorKind::PermissionDenied => {
                    self.report_err(t!("vault.error.basis_unlock_error", xous::LANG), None::<std::io::Error>)
                },
                _ => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
            }
        }
    }

    pub(crate) fn manage_basis(&mut self) {
        let mut bases = self.pddb.borrow().list_basis();
        bases.retain(|name| name != pddb::PDDB_DEFAULT_SYSTEM_BASIS);
        let b: Vec<&str> = bases.iter().map(AsRef::as_ref).collect();
        if bases.len() > 0 {
            self.modals
                .add_list(
                    b
                ).expect("couldn't create unmount modal");
            match self.modals.get_checkbox(t!("vault.basis.unmount", xous::LANG)) {
                Ok(unmount) => {
                    for b in unmount {
                        match self.pddb.borrow().lock_basis(&b) {
                            Ok(_) => log::debug!("basis {} locked", b),
                            Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                        }
                    }
                }
                Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
            }
        } else {
            if self.yes_no_approval(t!("vault.basis.none", xous::LANG)) {
            let name = match self.modals
                .alert_builder(t!("vault.basis.create", xous::LANG))
                .field(None, Some(name_validator))
                .build()
            {
                Ok(text) => {
                    text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
                },
                Err(e) => {self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); return}
            };
            self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
            match self.pddb.borrow().create_basis(&name) {
                Ok(_) => {
                    if self.yes_no_approval(t!("vault.basis.created_mount", xous::LANG)) {
                        match self.pddb.borrow().unlock_basis(&name, Some(BasisRetentionPolicy::Persist)) {
                            Ok(_) => log::debug!("Basis {} unlocked", name),
                            Err(e) => match e.kind() {
                                ErrorKind::PermissionDenied => {
                                    self.report_err(t!("vault.error.basis_unlock_error", xous::LANG), None::<std::io::Error>)
                                },
                                _ => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                            }
                        }
                    } else {
                        // do nothing
                    }
                }
                Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
            }
        } else {
                // do nothing
            }
        }
    }

    #[cfg(feature="testing")]
    pub(crate) fn populate_tests(&mut self) {
        use crate::ux::serialize_app_info;

        self.modals.dynamic_notification(Some("Creating test entries..."), None).ok();
        let words = [
            "bunnie", "foo", "turtle.net", "Fox.ng", "Bear", "dog food", "Cat.com", "FUzzy", "1off", "www_test_site_com/long_name/stupid/foo.htm",
            "._weird~yy%\":'test", "//WHYwhyWHY", "Xyz|zy", "foo:bar", "f🍕🍔🍟🌭d", "💎🙌", "some ノート", "笔录4u", "@u", "sane text", "Käsesoßenrührlöffel"];
        let weights = [1; 21];
        const TARGET_ENTRIES: usize = 35;
        const TARGET_ENTRIES_PW: usize = 100;
        // for each database, populate up to TARGET_ENTRIES
        // as this is testing code, it's written a bit more fragile in terms of error handling (fail-panic, rather than fail-dialog)
        // --- passwords ---
        let pws = self.pddb.borrow().list_keys(VAULT_PASSWORD_DICT, None).unwrap_or(Vec::new());
        if pws.len() < TARGET_ENTRIES_PW {
            let extra_count = TARGET_ENTRIES_PW - pws.len();
            for index in 0..extra_count {
                let desc = random_pick::pick_multiple_from_slice(&words, &weights, 3);
                let description = format!("{} {} {}", desc[0], desc[1], desc[2]);
                let username = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let notes = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
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
                let password = pg.generate_one().unwrap();
                let record = PasswordRecord {
                    version: VAULT_PASSWORD_REC_VERSION,
                    description,
                    username,
                    password,
                    notes,
                    ctime: utc_now().timestamp() as u64,
                    atime: 0,
                    count: 0,
                };
                let ser = serialize_password(&record);
                let guid = self.gen_guid();
                match self.pddb.borrow().get(
                    VAULT_PASSWORD_DICT,
                    &guid,
                    None, true, true,
                    Some(VAULT_ALLOC_HINT), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        match data.write(&ser) {
                            Ok(len) => {
                                log::debug!("pw wrote {} bytes", len);
                                self.modals.dynamic_notification_update(Some(&format!("pw entry {}, {} bytes", index, len)), None).ok();
                            },
                            Err(e) => log::error!("PW Error: {:?}", e),
                        }
                    }
                    Err(e) => log::error!("PW Error: {:?}", e),
                }
            }
        }
        // --- U2F + FIDO ---
        let fido = self.pddb.borrow().list_keys(FIDO_CRED_DICT, None).unwrap_or(Vec::new());
        let u2f = self.pddb.borrow().list_keys(U2F_APP_DICT, None).unwrap_or(Vec::new());
        let total = fido.len() + u2f.len();
        if total < TARGET_ENTRIES {
            let extra_u2f = (TARGET_ENTRIES - total) / 2;
            let extra_fido = TARGET_ENTRIES - extra_u2f;
            for index in 0..extra_u2f {
                let n = random_pick::pick_multiple_from_slice(&words, &weights, 2);
                let name = format!("{} {}", n[0], n[1]);
                let notes = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let mut id = [0u8; 32];
                self.trng.borrow_mut().fill_bytes(&mut id);
                let record = crate::AppInfo {
                    name,
                    id,
                    notes,
                    ctime: utc_now().timestamp() as u64,
                    atime: 0,
                    count: 0,
                };
                let ser = serialize_app_info(&record);
                let app_id_str = hex::encode(id);
                match self.pddb.borrow().get(
                    U2F_APP_DICT,
                    &app_id_str,
                    None, true, true,
                    Some(256), Some(crate::basis_change)
                ) {
                    Ok(mut app_data) => {
                        match app_data.write(&ser) {
                            Ok(len) => {
                                log::debug!("u2f wrote {} bytes", len);
                                self.modals.dynamic_notification_update(Some(&format!("u2f entry {}, {} bytes", index, len)), None).ok();
                            }
                            Err(e) => log::error!("U2F Error: {:?}", e),
                        }
                    }
                    _ => log::error!("U2F Error creating record"),
                }
            }
            let xns = xous_names::XousNames::new().unwrap();
            let mut rng = ctap_crypto::rng256::XousRng256::new(&xns);
            for index in 0..extra_fido {
                use crate::ctap::data_formats::*;
                let c_id = random_pick::pick_multiple_from_slice(&words, &weights, 2);
                let cred_id = format!("{} {} {}", c_id[0], c_id[1], index);
                let r_id = random_pick::pick_multiple_from_slice(&words, &weights, 2);
                let rp_id = format!("{} {}", r_id[0], r_id[1]);
                let handle = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let new_credential = PublicKeyCredentialSource {
                    key_type: PublicKeyCredentialType::PublicKey,
                    credential_id: cred_id.as_bytes().to_vec(),
                    private_key: ctap_crypto::ecdsa::SecKey::gensk(&mut rng),
                    rp_id,
                    user_handle: handle.as_bytes().to_vec(),
                    user_display_name: None,
                    cred_protect_policy: None,
                    creation_order: 0,
                    user_name: None,
                    user_icon: None,
                };
                let shortid = &cred_id;
                match self.pddb.borrow().get(
                    FIDO_CRED_DICT,
                    shortid,
                    None, true, true,
                    Some(crate::ctap::storage::CRED_INITAL_SIZE), Some(crate::basis_change)
                ) {
                    Ok(mut cred) => {
                        let value = crate::ctap::storage::serialize_credential(new_credential).unwrap();
                        match cred.write(&value) {
                            Ok(len) => {
                                log::debug!("fido2 wrote {} bytes", len);
                                self.modals.dynamic_notification_update(Some(&format!("fido2 entry {}, {} bytes", index, len)), None).ok();
                            }
                            Err(e) => log::error!("FIDO2 Error: {:?}", e),
                        }
                    }
                    _ => log::error!("couldn't create FIDO2 credential")
                }
            }
        }
        // TOTP
        let totp = self.pddb.borrow().list_keys(VAULT_TOTP_DICT, None).unwrap_or(Vec::new());
        if totp.len() < TARGET_ENTRIES {
            let extra = TARGET_ENTRIES - totp.len();
            for index in 0..extra {
                let names = random_pick::pick_multiple_from_slice(&words, &weights, 3);
                let name = format!("{} {} {}", names[0], names[1], names[2]);
                let notes = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let mut secret_bytes = [0u8; 10];
                self.trng.borrow_mut().fill_bytes(&mut secret_bytes);
                let record = TotpRecord {
                    version: VAULT_TOTP_REC_VERSION,
                    secret: base32::encode(base32::Alphabet::RFC4648 { padding: false }, &secret_bytes),
                    name,
                    algorithm: TotpAlgorithm::HmacSha1,
                    notes,
                    digits: 6,
                    timestep: 30,
                    ctime: utc_now().timestamp() as u64,
                };
                let ser = serialize_totp(&record);
                let guid = self.gen_guid();
                match self.pddb.borrow().get(
                    VAULT_TOTP_DICT,
                    &guid,
                    None, true, true,
                    Some(VAULT_TOTP_ALLOC_HINT), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        match data.write(&ser) {
                            Ok(len) => {
                                self.modals.dynamic_notification_update(Some(&format!("totp entry {}, {} bytes", index, len)), None).ok();
                                log::debug!("totp wrote {} bytes", len);
                            },
                            Err(e) => log::error!("TOTP Error: {:?}", e),
                        }
                    }
                    Err(e) => log::error!("TOTP Error: {:?}", e),
                }
            }
            // specific TOTP entry with a known shared secret for testing
            let record = TotpRecord {
                version: VAULT_TOTP_REC_VERSION,
                secret: "I65VU7K5ZQL7WB4E".to_string(),
                name: "totp@authenticationtest.com".to_string(),
                algorithm: TotpAlgorithm::HmacSha1,
                notes: "Predefined test".to_string(),
                digits: 6,
                timestep: 30,
                ctime: utc_now().timestamp() as u64,
            };
            let ser = serialize_totp(&record);
            let guid = self.gen_guid();
            match self.pddb.borrow().get(
                VAULT_TOTP_DICT,
                &guid,
                None, true, true,
                Some(VAULT_TOTP_ALLOC_HINT), Some(crate::basis_change)
            ) {
                Ok(mut data) => {
                    match data.write(&ser) {
                        Ok(len) => {
                            self.modals.dynamic_notification_update(Some(&format!("totp entry hardcoded, {} bytes", len)), None).ok();
                            log::debug!("totp wrote {} bytes", len);
                        },
                        Err(e) => log::error!("TOTP Error: {:?}", e),
                    }
                }
                Err(e) => log::error!("TOTP Error: {:?}", e),
            }
        }
        self.modals.dynamic_notification_update(Some("Syncing PDDB..."), None).ok();
        self.pddb.borrow().sync().ok();
        self.modals.dynamic_notification_close().ok();
    }

    fn report_err<T: std::fmt::Debug>(&self, note: &str, e: Option<T>) {
        log::error!("{}: {:?}", note, e);
        if let Some(e) = e {
            self.modals.show_notification(&format!("{}\n{:?}", note, e), None).ok();
        } else {
            self.modals.show_notification(&format!("{}", note), None).ok();
        }
    }
}


pub(crate) fn totp_ss_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let proposed_ss = input.as_str().to_uppercase();
    if let Some(ss) = base32::decode(base32::Alphabet::RFC4648 { padding: false }, &proposed_ss) {
        if ss.len() > 0 {
            return None;
        } else {
            return Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_totp", xous::LANG)));
        }
    }
    if let Some(ss) = base32::decode(base32::Alphabet::RFC4648 { padding: true }, &proposed_ss) {
        if ss.len() > 0 {
            return None;
        } else {
            return Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_totp", xous::LANG)));
        }
    }
    if let Some(ss) = base32::decode(base32::Alphabet::Crockford, &proposed_ss) {
        if ss.len() > 0 {
            return None;
        } else {
            return Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_totp", xous::LANG)));
        }
    }
    Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_totp", xous::LANG)))
}
pub(crate) fn name_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let proposed_name = input.as_str();
    if proposed_name.contains(['\n',':']) { // the '\n' is reserved as the delimiter to end the name field, and ':' is the path separator
        Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_char", xous::LANG)))
    } else {
        None
    }
}
pub(crate) fn password_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let proposed_name = input.as_str();
    if proposed_name.contains(['\n']) { // the '\n' is reserved as the delimiter to end the name field
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
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
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
                            log::warn!("ver error");
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
                            log::warn!("ctime error");
                            return None;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            pr.atime = atime;
                        } else {
                            log::warn!("atime error");
                            return None;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            pr.count = count;
                        } else {
                            log::warn!("count error");
                            return None;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing password info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        Some(pr)
    } else {
        None
    }
}

pub(crate) fn serialize_totp<'a>(record: &TotpRecord) -> Vec::<u8> {
    let ta: String = record.algorithm.into();
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
        "version", record.version,
        "secret", record.secret,
        "name", record.name,
        "algorithm", ta,
        "notes", record.notes,
        "digits", record.digits,
        "timestep", record.timestep,
        "ctime", record.ctime,
    ).into_bytes()
}

pub(crate) fn deserialize_totp(data: Vec::<u8>) -> Option<TotpRecord> {
    if let Ok(desc_str) = String::from_utf8(data) {
        let mut pr = TotpRecord {
            version: 0,
            secret: String::new(),
            name: String::new(),
            algorithm: TotpAlgorithm::HmacSha1,
            notes: String::new(),
            digits: 0,
            ctime: 0,
            timestep: 0,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "version" => {
                        if let Ok(ver) = u32::from_str_radix(data, 10) {
                            pr.version = ver
                        } else {
                            log::warn!("ver error");
                            return None;
                        }
                    }
                    "secret" => pr.secret.push_str(data),
                    "name" => pr.name.push_str(data),
                    "algorithm" => pr.algorithm = match TotpAlgorithm::try_from(data) {
                        Ok(a) => a,
                        Err(_) => return None
                    },
                    "notes" => pr.notes.push_str(data),
                    "digits" => {
                        if let Ok(digits) = u32::from_str_radix(data, 10) {
                            pr.digits = digits;
                        } else {
                            log::warn!("digits error");
                            return None;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            pr.ctime = ctime;
                        } else {
                            log::warn!("ctime error");
                            return None;
                        }
                    }
                    "timestep" => {
                        if let Ok(timestep) = u64::from_str_radix(data, 10) {
                            pr.timestep = timestep;
                        } else {
                            log::warn!("timestep error");
                            return None;
                        }
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing TOTP info, ignoring", tag);
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
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
