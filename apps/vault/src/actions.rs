use core::convert::TryFrom;
use std::io::ErrorKind;
use gam::TextEntryPayload;
use pddb::BasisRetentionPolicy;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use num_traits::*;
use xous::{Message, send_message};
use locales::t;
use std::io::{Write, Read};
use passwords::PasswordGenerator;
use std::cell::RefCell;

use vault::{
    deserialize_app_info, serialize_app_info, basis_change,
    VAULT_PASSWORD_DICT, VAULT_TOTP_DICT, VAULT_ALLOC_HINT, utc_now,
    atime_to_str
};
use crate::ListItem;
use crate::{ItemLists, VaultMode, SelectedEntry};
use crate::storage::{self, PasswordRecord, StorageContent};
use crate::totp::TotpAlgorithm;
use persistent_store::store::OPENSK2_DICT;

use vault::env::xous::U2F_APP_DICT;

#[cfg(feature="vaultperf")]
use perflib::*;
#[cfg(feature="vaultperf")]
const FILE_ID_APPS_VAULT_SRC_ACTIONS: u32 = 1;

const VAULT_PASSWORD_REC_VERSION: u32 = 1;
const VAULT_TOTP_REC_VERSION: u32 = 1;
/// time allowed between dialog box swaps for background operations to redraw
#[cfg(feature="ux-swap-delay")]
const SWAP_DELAY_MS: usize = 300;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ActionOp {
    /// Menu items
    MenuAddnew,
    MenuEditStage2,
    MenuDeleteStage2,
    MenuClose,
    MenuUnlockBasis,
    MenuManageBasis,
    /// Internal ops
    UpdateMode,
    UpdateOneItem,
    Quit,
    #[cfg(feature="vault-testing")]
    /// Testing
    GenerateTests,
}
/*
pub(crate) fn start_actions_thread<'a> (
    main_conn: xous::CID,
    sid: SID, mode: Arc::<Mutex::<VaultMode>>,
    item_lists: Arc::<Mutex::<ItemLists<'a>>>,
    action_active: Arc::<AtomicBool>,
    opensk_mutex: Arc::<Mutex::<i32>>,
) {
    let _ = thread::spawn({
        move || {
            let mut manager = ActionManager::new(main_conn, mode, item_lists, action_active, opensk_mutex);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                let opcode: Option<ActionOp> = FromPrimitive::from_usize(msg.body.id());
                log::debug!("{:?}", opcode);
                match opcode {
                    Some(ActionOp::MenuAddnew) => {
                        manager.activate();
                        manager.menu_addnew();
                        // this is necessary so the next redraw shows the newly added entry
                        // no cache clear is called for because new entries will always add to the list;
                        // there is no risk of "stale" entries persisting
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
                        manager.item_lists.lock().unwrap().pw.clear(); // clear the cached item list for passwords (totp/fido are not cached and don't need clearing)
                        manager.retrieve_db();
                        manager.deactivate();
                    },
                    Some(ActionOp::MenuManageBasis) => {
                        manager.activate();
                        manager.manage_basis();
                        manager.item_lists.lock().unwrap().pw.clear(); // clear the cached item list for passwords
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
}
*/

pub struct ActionManager<'a> {
    modals: modals::Modals,
    storage: RefCell<storage::Manager>,

    #[cfg(feature="vault-testing")]
    trng: RefCell::<trng::Trng>,

    mode: Arc::<Mutex::<VaultMode>>,
    pub item_lists: Arc::<Mutex::<ItemLists>>,
    pddb: RefCell::<pddb::Pddb>,
    tt: ticktimer_server::Ticktimer,
    action_active: Arc::<AtomicBool>,
    opensk_mutex: Arc::<Mutex::<i32>>,
    mode_cache: VaultMode,
    main_conn: xous::CID,
    #[cfg(feature="vaultperf")]
    perfbuf: xous::MemoryRange,
    #[cfg(feature="vaultperf")]
    pm: PerfMgr<'a>,
    #[cfg(feature="vaultperf")]
    pid: u32,
    // this is necessary to keep rustc quiet when not using `vaultperf` build option...
    phantom: core::marker::PhantomData<&'a u32>,
}
impl<'a> ActionManager<'a> {
    pub fn new(
        main_conn: xous::CID,
        mode: Arc::<Mutex::<VaultMode>>,
        item_lists: Arc::<Mutex::<ItemLists>>,
        action_active: Arc::<AtomicBool>,
        opensk_mutex: Arc::<Mutex::<i32>>,
    ) -> ActionManager<'a> {
        let xns = xous_names::XousNames::new().unwrap();
        let storage_manager = storage::Manager::new(&xns);

        // notes: to use vault as the performance manager, build with `cargo xtask perf-image vault --feature vaultperf`.
        // this will override shellchat as the performance manager, while enabling all the other performance reporting agents
        #[cfg(feature="vaultperf")]
        let perfbuf = xous::syscall::map_memory(
            None,
            None,
            BUFLEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE,
        ).expect("couldn't map in the performance buffer");
        #[cfg(feature="vaultperf")]
        let pm = build_perf_mgr(perfbuf.as_mut_ptr());

        let mc = (*mode.lock().unwrap()).clone();
        ActionManager {
            modals: modals::Modals::new(&xns).unwrap(),
            storage: RefCell::new(storage_manager),

            #[cfg(feature="vault-testing")]
            trng: RefCell::new(trng::Trng::new(&xns).unwrap()),

            mode_cache: mc,
            mode,
            item_lists,
            pddb: RefCell::new(pddb::Pddb::new()),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
            action_active,
            opensk_mutex,
            main_conn,
            #[cfg(feature="vaultperf")]
            perfbuf,
            #[cfg(feature="vaultperf")]
            pm,
            #[cfg(feature="vaultperf")]
            pid: xous::process::id() as u32,
            phantom: core::marker::PhantomData,
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
        #[cfg(feature="ux-swap-delay")]
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
                #[cfg(feature="ux-swap-delay")]
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let username = match self.modals
                    .alert_builder(t!("vault.newitem.username", xous::LANG))
                    .field(None, Some(password_validator))
                    .build()
                {
                    Ok(text) => text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string(),
                    _ => {log::error!("Name entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                };
                #[cfg(feature="ux-swap-delay")]
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let mut approved = false;
                let mut bip39 = false;
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
                    #[cfg(feature="ux-swap-delay")]
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
                        #[cfg(feature="ux-swap-delay")]
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
                        #[cfg(feature="ux-swap-delay")]
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
                    } else if maybe_password == "bip39" {
                        bip39 = true;
                        approved = true;
                        match self.modals.input_bip39(Some(t!("vault.bip39.input", xous::LANG))) {
                            Ok(data) => hex::encode(data),
                            _ => "".to_string(),
                        }
                    } else {
                        approved = true;
                        maybe_password
                    };
                }
                let mut record = storage::PasswordRecord {
                    version: VAULT_PASSWORD_REC_VERSION,
                    description,
                    username,
                    password,
                    notes: if bip39 { "bip39".to_string() } else {t!("vault.notes", xous::LANG).to_string()},
                    ctime: 0,
                    atime: 0,
                    count: 0,
                };

                match self.storage.borrow_mut().new_record(&mut record, None, true) {
                    Ok(_) => (),
                    Err(error) => {
                        log::error!("internal error");
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                    },
                };
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

                #[cfg(feature="ux-swap-delay")]
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                self.modals
                    .add_list(vec![
                        t!("vault.newitem.totp", xous::LANG),
                        t!("vault.newitem.hotp", xous::LANG),
                    ]).expect("couldn't create configuration modal");
                let is_totp: bool;
                match self.modals.get_radiobutton(t!("vault.newitem.is_t_or_h_otp", xous::LANG)) {
                    Ok(response) => {
                        if &response == t!("vault.newitem.totp", xous::LANG) {
                            is_totp = true;
                        } else {
                            is_totp = false;
                        }
                    }
                    _ => {log::error!("Modal selection error"); self.action_active.store(false, Ordering::SeqCst); return}
                }

                #[cfg(feature="ux-swap-delay")]
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
                #[cfg(feature="ux-swap-delay")]
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

                let timestep = if !is_totp {
                    // get the initial count if it's an HOTP record
                    #[cfg(feature="ux-swap-delay")]
                    self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                    match self.modals
                        .alert_builder(t!("vault.hotp.count", xous::LANG))
                        .field(Some("0".to_string()), Some(count_validator))
                        .build()
                    {
                        Ok(entry) => entry.content()[0].content.as_str().unwrap().parse::<u64>().unwrap(),
                        _ => {log::error!("Count entry failed"); self.action_active.store(false, Ordering::SeqCst); return}
                    }
                } else {
                    30 // default TOTP timestep otherwise
                };

                // time, hash, etc. are all the "expected defaults" -- if you want to change them, edit the record after entering it.
                let mut totp = storage::TotpRecord {
                    version: VAULT_TOTP_REC_VERSION,
                    name: description,
                    secret: validated_secret,
                    algorithm: TotpAlgorithm::HmacSha1,
                    digits: 6,
                    timestep,
                    ctime: 0,
                    is_hotp: !is_totp,
                    notes: t!("vault.notes", xous::LANG).to_string(),
                };

                match self.storage.borrow_mut().new_record(&mut totp, None, true) {
                    Ok(_) => (),
                    Err(error) => {
                        log::error!("internal error");
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                    },
                };
            }
        }
    }

    pub(crate) fn menu_delete(&mut self, entry: SelectedEntry) {
        if self.yes_no_approval(&format!("{}\n{}", t!("vault.delete.confirm", xous::LANG), entry.description)) {
            let choice = match entry.mode {
                VaultMode::Password => Some(storage::ContentKind::Password),
                VaultMode::Totp => Some(storage::ContentKind::TOTP),
                VaultMode::Fido => None,
            };

            if choice.is_none() {
                // we're dealing with FIDO stuff, use the custom code path
                let dictionary = match usize::from_str_radix(entry.key_name.as_str().unwrap_or("UTF8-error"), 10) {
                    Ok(fido_key) => {
                        if vault::ctap::storage::key::CREDENTIALS.contains(&fido_key) {
                            persistent_store::store::OPENSK2_DICT // heuristic: all fido2 keys are simple integers
                        } else {
                            U2F_APP_DICT
                        }
                    }
                    Err(_) => U2F_APP_DICT, // u2f keys are long hex strings
                };
                match self.pddb.borrow().get(dictionary, entry.key_name.as_str().unwrap_or("UTF8-error"),
                None, false, false, None, None::<fn()>
                ) {
                    Ok(candidate) => {
                        let attr = candidate.attributes().expect("couldn't get key attributes");
                        match self.pddb.borrow()
                        .delete_key(
                            dictionary,
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
                // we're deleting either a password, or a totp
                let choice = choice.unwrap();
                let guid = entry.key_name.as_str().unwrap_or("UTF8-error");
                if entry.mode == VaultMode::Password {
                    // self.modals.show_notification(&format!("deleting key {}", guid), None).ok();
                    // if it's a password, we have to pull the full record, and then reconstitute the item_lists index key so we can remove it from the UX cache
                    let storage = self.storage.borrow_mut();
                    let pw: storage::PasswordRecord =  match storage.get_record(&choice, guid) {
                        Ok(record) => record,
                        Err(error) => {
                            self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                            return;
                        }
                    };
                    let mut desc = String::with_capacity(256);
                    make_pw_name(&pw.description, &pw.username, &mut desc);
                    let key = ListItem::key_from_parts(&desc, guid);
                    self.item_lists.lock().unwrap().remove(entry.mode, key);
                    /*
                    if self.item_lists.lock().unwrap().pw.remove(&key).is_some() {
                        self.modals.show_notification(&format!("deleted UX {}", &key), None).ok();
                    } else {
                        self.modals.show_notification(&format!("could not delete UX {}", &key), None).ok();
                    }; */
                }
                match self.storage.borrow_mut().delete(choice, guid) {
                    Ok(_) => self.modals.show_notification(t!("vault.completed", xous::LANG), None).ok().unwrap(),
                    Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                }
            }
            self.pddb.borrow().sync().ok();
        }
    }

    /// Update UX cached data for just one entry
    pub(crate) fn update_db_entry(&mut self, entry: SelectedEntry) {
        match entry.mode {
            VaultMode::Password => {
                let choice = storage::ContentKind::Password;
                let guid = entry.key_name.as_str().unwrap_or("UTF8-error");
                let storage = self.storage.borrow_mut();
                let pw: storage::PasswordRecord =  match storage.get_record(&choice, guid) {
                    Ok(record) => record,
                    Err(error) => {
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                        return;
                    }
                };
                // create the list item from the updated entry
                let mut desc = String::with_capacity(256);
                make_pw_name(&pw.description, &pw.username, &mut desc);
                let mut extra = String::with_capacity(256);
                let human_time = atime_to_str(pw.atime);
                extra.push_str(&human_time);
                extra.push_str("; ");
                extra.push_str(t!("vault.u2f.appinfo.authcount", xous::LANG));
                extra.push_str(&pw.count.to_string());
                let li = ListItem {
                    name: desc.to_string(), // these allocs will be slow, but we do it only once on boot
                    extra: extra.to_string(),
                    dirty: true,
                    guid: guid.to_string(),
                    atime: pw.atime,
                    count: pw.count,
                };
                log::debug!("updating {} to list item {}", li.extra, li.key());
                assert!(
                    self.item_lists.lock().unwrap().insert(entry.mode, li.key(), li).is_some(),
                    "Somehow, the autotyped record isn't in the UX list for updating!")
                ;
            },
            _ => {
                // no cached data, no action
            }
        };
    }

    pub(crate) fn menu_edit(&mut self, entry: SelectedEntry) {
        let choice = match entry.mode {
            VaultMode::Password => Some(storage::ContentKind::Password),
            VaultMode::Totp => Some(storage::ContentKind::TOTP),
            VaultMode::Fido => None,
        };

        if choice.is_none() {
            let dict = U2F_APP_DICT;
            // at the moment only U2F records are supported for editing. The FIDO2 stuff is done with a different record
            // storage format that's a bit funkier to edit.
            let maybe_update = match self.pddb.borrow().get(
                dict, entry.key_name.as_str().unwrap(), None,
                false, false, None, Some(basis_change)
            ) {
                Ok(mut record) => {
                    // resolve the basis of the key, so that we are editing it "in place"
                    let attr = record.attributes().expect("couldn't get key attributes");
                    let mut data = Vec::<u8>::new();
                    let maybe_update = match record.read_to_end(&mut data) {
                        Ok(_len) => {
                            if let Some(mut ai) = deserialize_app_info(data) {
                                let edit_data = if ai.notes != t!("vault.notes", xous::LANG) {
                                    self.modals
                                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                    .field_placeholder_persist(Some(ai.name), Some(password_validator))
                                    .field_placeholder_persist(Some(ai.notes), Some(password_validator))
                                    .field_placeholder_persist(Some(hex::encode(ai.id)), None)
                                    .build().expect("modals error in edit")
                                } else {
                                    self.modals
                                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                                    .field_placeholder_persist(Some(ai.name), Some(password_validator))
                                    .field(Some(ai.notes), Some(password_validator))
                                    .field_placeholder_persist(Some(hex::encode(ai.id)), None)
                                    .build().expect("modals error in edit")
                                };
                                ai.name = edit_data.content()[0].content.as_str().unwrap().to_string();
                                ai.notes = edit_data.content()[1].content.as_str().unwrap().to_string();
                                ai.atime = 0;
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
                    Some(basis_change)
                ) {
                    Ok(mut record) => {
                        let ser = serialize_app_info(&update);
                        record.write(&ser).unwrap_or_else(|e| {
                            self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)); 0});
                    }
                    Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
                }
            }
            self.pddb.borrow().sync().ok();
            return;
        }

        let choice = choice.unwrap();
        let key_name =  entry.key_name.as_str().unwrap();
        let mut storage = self.storage.borrow_mut();

        let maybe_edited = match choice {
            storage::ContentKind::TOTP => {
                let mut pw: storage::TotpRecord =  match storage.get_record(&choice, key_name) {
                    Ok(record) => record,
                    Err(error) => {
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                        return;
                    }
                };

                let edit_data = if pw.notes != t!("vault.notes", xous::LANG) {
                    self.modals
                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                    .field_placeholder_persist(Some(pw.name), Some(password_validator))
                    .field_placeholder_persist(Some(pw.secret), Some(password_validator))
                    .field_placeholder_persist(Some(pw.notes), Some(password_validator))
                    .field(Some(pw.timestep.to_string()), Some(password_validator))
                    .field(Some(pw.algorithm.to_string()), Some(password_validator))
                    .field(Some(pw.digits.to_string()), Some(password_validator))
                    .field(Some(if pw.is_hotp {"HOTP".to_string()} else {"TOTP".to_string()}), Some(password_validator))
                    .build().expect("modals error in edit")
                } else {
                    self.modals
                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                    .field_placeholder_persist(Some(pw.name), Some(password_validator))
                    .field_placeholder_persist(Some(pw.secret), Some(password_validator))
                    .field(Some(pw.notes), Some(password_validator))
                    .field(Some(pw.timestep.to_string()), Some(password_validator))
                    .field(Some(pw.algorithm.to_string()), Some(password_validator))
                    .field(Some(pw.digits.to_string()), Some(password_validator))
                    .field(Some(if pw.is_hotp {"HOTP".to_string()} else {"TOTP".to_string()}), Some(password_validator))
                    .build().expect("modals error in edit")
                };
                pw.name = edit_data.content()[0].content.as_str().unwrap().to_string();
                pw.secret = edit_data.content()[1].content.as_str().unwrap().to_string();
                pw.notes = edit_data.content()[2].content.as_str().unwrap().to_string();
                pw.is_hotp = if edit_data.content()[6].content.as_str().unwrap().to_string().to_uppercase() == "HOTP" {true} else {false};
                if let Ok(t) = u64::from_str_radix(edit_data.content()[3].content.as_str().unwrap(), 10) {
                    pw.timestep = t;
                }
                if let Ok(alg) = TotpAlgorithm::try_from(edit_data.content()[4].content.as_str().unwrap()) {
                    pw.algorithm = alg;
                }
                if let Ok(d) = u32::from_str_radix(edit_data.content()[5].content.as_str().unwrap(), 10) {
                    pw.digits = d;
                }

                storage.update(&choice, key_name, &mut pw)
            },
            storage::ContentKind::Password => {
                let mut pw: storage::PasswordRecord =  match storage.get_record(&choice, key_name) {
                    Ok(record) => record,
                    Err(error) => {
                        self.report_err(t!("vault.error.internal_error", xous::LANG), Some(error));
                        return;
                    }
                };
                // remove the entry from the old UX list
                let mut desc = String::new();
                make_pw_name(&pw.description, &pw.username, &mut desc);

                self.item_lists.lock().unwrap().remove(VaultMode::Password, ListItem::key_from_parts(&desc, key_name));

                // display previous data for edit
                let edit_data = if pw.notes != t!("vault.notes", xous::LANG) {
                    self.modals
                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                    .field_placeholder_persist(Some(pw.description), Some(password_validator))
                    .field_placeholder_persist(Some(pw.username), Some(password_validator))
                    .field_placeholder_persist(Some(pw.password), Some(password_validator))
                    .field_placeholder_persist(Some(pw.notes), Some(password_validator))
                    .set_growable()
                    .build().expect("modals error in edit")
                } else { // note is placeholder text, treat it as such
                self.modals
                    .alert_builder(t!("vault.edit_dialog", xous::LANG))
                    .field_placeholder_persist(Some(pw.description), Some(password_validator))
                    .field_placeholder_persist(Some(pw.username), Some(password_validator))
                    .field_placeholder_persist(Some(pw.password), Some(password_validator))
                    .field(Some(pw.notes), Some(password_validator))
                    .set_growable()
                    .build().expect("modals error in edit")
                };

                pw.description = edit_data.content()[0].content.as_str().unwrap().to_string();
                pw.username = edit_data.content()[1].content.as_str().unwrap().to_string();
                pw.password = edit_data.content()[2].content.as_str().unwrap().to_string();
                pw.notes = edit_data.content()[3].content.as_str().unwrap().to_string();

                // if the notes field starts with the word "bip39" (case insensitive), use BIP39 to display/edit the password field
                if pw.notes.to_ascii_lowercase().starts_with("bip39") {
                    if pw.password.len() == 0 {
                        match self.modals.input_bip39(Some(t!("vault.bip39.input", xous::LANG))) {
                            Ok(data) => {
                                pw.password = hex::encode(data);
                            }
                            _ => pw.password = "".to_string(), // leave it blank if invalid or aborted
                        }
                    } else {
                        match hex::decode(&pw.password) {
                            Ok(data) => {
                                match self.modals.show_bip39(
                                    Some(t!("vault.bip39.output", xous::LANG)),
                                    &data
                                ) {
                                    Ok(_) => {},
                                    Err(_) => {
                                        self.modals.show_notification(t!("vault.bip39.output_error", xous::LANG), None).unwrap();
                                    }
                                }
                            }
                            Err(_) => {
                                self.modals.show_notification(t!("vault.bip39.output_error", xous::LANG), None).unwrap();
                            }
                        }
                    }
                } else if pw.password.len() == 0 && !pw.notes.to_ascii_lowercase().starts_with("bip39") {
                    // if the password is empty, prompt to generate a new password
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
                    let mut approved = false;
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
                        #[cfg(feature="ux-swap-delay")]
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
                            #[cfg(feature="ux-swap-delay")]
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
                            #[cfg(feature="ux-swap-delay")]
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
                    pw.password = password;
                }

                // note the edit access, this counts as an access since the password was revealed
                pw.count += 1;
                pw.atime = utc_now().timestamp() as u64;
                storage.update(&choice, key_name, &mut pw)
           },
        };

        match maybe_edited {
            Ok(_) => {},
            Err(e) => self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e)),
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

    #[cfg(feature="vaultperf")]
    #[inline]
    /// create performance logger entries
    pub fn perfentry(&self, pm: &PerfMgr, meta: u32, index: u32, line: u32) {
        let event = perf_entry!(self.pid, FILE_ID_APPS_VAULT_SRC_ACTIONS, meta, index, line);
        pm.log_event_unchecked(event);
    }

    /// Populate the display list with data from the PDDB. Limited by total available RAM; probably
    /// would stop working if you have over 500-1k records with the current heap limits.
    ///
    /// This has been performance-optimized for our platform:
    ///   - `format!()` is very slow, so we use `push_str()` where possible
    ///   - allocations are slow, so we try to avoid them at all costs
    pub(crate) fn retrieve_db(&mut self) {
        #[cfg(feature="vaultperf")]
        self.pm.stop_and_reset();
        #[cfg(feature="vaultperf")]
        self.pm.start();
        #[cfg(feature="vaultperf")]
        self.perfentry(&self.pm, PERFMETA_STARTBLOCK, 0, std::line!());

        self.mode_cache = {
            (*self.mode.lock().unwrap()).clone()
        };
        log::debug!("heap usage A: {}", heap_usage());
        match self.mode_cache {
            VaultMode::Password => {
                let start = self.tt.elapsed_ms();
                #[cfg(feature="vaultperf")]
                self.perfentry(&self.pm, PERFMETA_STARTBLOCK, 1, std::line!());
                let mut klen = 0;
                match self.pddb.borrow().read_dict(VAULT_PASSWORD_DICT, None, Some(256 * 1024)) {
                    Ok(keys) => {
                        #[cfg(feature="vaultperf")]
                        self.perfentry(&self.pm, PERFMETA_NONE, 1, std::line!());
                        let mut oom_keys = 0;
                        // allocate a re-usable temporary buffers, to avoid triggering allocs
                        let mut pw_rec = PasswordRecord::alloc();
                        let mut extra = String::with_capacity(256);
                        let mut desc = String::with_capacity(256);
                        let mut lookup_key = String::with_capacity(384);
                        let il = self.item_lists.lock().unwrap();
                        for key in keys {
                            #[cfg(feature="vaultperf")]
                            self.perfentry(&self.pm, PERFMETA_STARTBLOCK, 2, std::line!());
                            if let Some(data) = key.data {
                                if pw_rec.from_vec(data).is_ok() {
                                    // reset the re-usable structures
                                    lookup_key.clear();
                                    extra.clear();
                                    #[cfg(feature="vaultperf")]
                                    self.perfentry(&self.pm, PERFMETA_NONE, 2, std::line!());

                                    // build the description string
                                    make_pw_name(&pw_rec.description, &pw_rec.username, &mut desc);

                                    // build the storage key in the list array
                                    lookup_key.push_str(&desc);
                                    lookup_key.push_str(&key.name);

                                    #[cfg(feature="vaultperf")]
                                    self.perfentry(&self.pm, PERFMETA_NONE, 2, std::line!());
                                    if let Some(prev_entry) = il.get(self.mode_cache, &lookup_key) {
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_STARTBLOCK, 3, std::line!());
                                        prev_entry.lock().unwrap().dirty = true;
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_NONE, 3, std::line!());
                                        if prev_entry.lock().unwrap().atime != pw_rec.atime || prev_entry.lock().unwrap().count != pw_rec.count {
                                            // this is expensive, so don't run it unless we have to
                                            let human_time = atime_to_str(pw_rec.atime);
                                            // note this code is duplicated in update_db_entry()
                                            extra.push_str(&human_time);
                                            extra.push_str("; ");
                                            extra.push_str(t!("vault.u2f.appinfo.authcount", xous::LANG));
                                            extra.push_str(&pw_rec.count.to_string());
                                            prev_entry.lock().unwrap().extra.clear();
                                            prev_entry.lock().unwrap().extra.push_str(&extra);
                                        }
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_NONE, 3, std::line!());
                                        if prev_entry.lock().unwrap().name != desc { // this check should be redundant, but, leave it in to be safe
                                            prev_entry.lock().unwrap().name.clear();
                                            prev_entry.lock().unwrap().name.push_str(&desc);
                                        }
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_NONE, 3, std::line!());
                                        if prev_entry.lock().unwrap().guid != key.name {
                                            prev_entry.lock().unwrap().guid.clear();
                                            prev_entry.lock().unwrap().guid.push_str(&key.name);
                                        }
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_ENDBLOCK, 3, std::line!());
                                    } else {
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_STARTBLOCK, 4, std::line!());

                                        let human_time = atime_to_str(pw_rec.atime);

                                        extra.push_str(&human_time);
                                        extra.push_str("; ");
                                        extra.push_str(t!("vault.u2f.appinfo.authcount", xous::LANG));
                                        extra.push_str(&pw_rec.count.to_string());

                                        let li = ListItem {
                                            name: desc.to_string(), // these allocs will be slow, but we do it only once on boot
                                            extra: extra.to_string(),
                                            dirty: true,
                                            guid: key.name,
                                            atime: pw_rec.atime,
                                            count: pw_rec.count,
                                        };
                                        self.item_lists.lock().unwrap()
                                        .insert(self.mode_cache, li.key(), li); // this push is very slow, but we only have to do it once on boot
                                        #[cfg(feature="vaultperf")]
                                        self.perfentry(&self.pm, PERFMETA_ENDBLOCK, 4, std::line!());
                                    }

                                    klen += 1;
                                } else {
                                    log::warn!("Couldn't interpret password record: {}", key.name);
                                }
                            } else {
                                /* // this code helps to trace down which key had OOM'd, if it turns out to be an issue
                                let mut oom_key = String::new();
                                make_pw_name(&pw_rec.description, &pw_rec.username, &mut oom_key);
                                let li = ListItem {
                                    name: oom_key.to_string(),
                                    extra: "Maybe OOM record".to_string(),
                                    dirty: true,
                                    guid: key.name,
                                    atime: 0,
                                    count: 0,
                                };
                                il.insert(li.key(), li); // this push is very slow, but we only have to do it once on boot
                                */
                                oom_keys += 1;
                            }
                            #[cfg(feature="vaultperf")]
                            self.perfentry(&self.pm, PERFMETA_ENDBLOCK, 2, std::line!());
                        }
                        if oom_keys != 0 {
                            log::warn!("Ran out of cache space handling password keys. {} keys are not loaded.", oom_keys);
                            self.report_err(&format!("Ran out of cache space handling passwords. {} passwords not loaded", oom_keys), None::<crate::storage::Error>);
                        }
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::NotFound => {
                                // this is fine, it just means no passwords have been entered yet
                            },
                            _ => {
                                log::error!("Error opening password dictionary");
                                self.report_err("Error opening password dictionary", Some(e))
                            }
                        }
                    },
                }
                #[cfg(feature="vaultperf")]
                self.perfentry(&self.pm, PERFMETA_ENDBLOCK, 1, std::line!());
                log::info!("readout took {} ms for {} elements", self.tt.elapsed_ms() - start, klen);
            }
            VaultMode::Fido => {
                // first assemble U2F records
                log::debug!("listing in {}", U2F_APP_DICT);
                // regen from scratch every time, It's slow, but we're counting on <20 FIDO entries on average
                self.item_lists.lock().unwrap().clear(self.mode_cache);
                match self.pddb.borrow().read_dict(U2F_APP_DICT, None, Some(256 * 1024)) {
                    Ok(keys) => {
                        let mut oom_keys = 0;
                        for key in keys {
                            if let Some(data) = key.data {
                                if let Some(ai) = deserialize_app_info(data) {
                                    let extra = format!("{}; {}{}",
                                        atime_to_str(ai.atime),
                                        t!("vault.u2f.appinfo.authcount", xous::LANG),
                                        ai.count,
                                    );
                                    let desc = format!("{} (U2F)", ai.name);
                                    let li = ListItem {
                                        name: desc,
                                        extra,
                                        dirty: true,
                                        guid: key.name,
                                        count: ai.count,
                                        atime: ai.atime,
                                    };
                                    self.item_lists.lock().unwrap().insert(self.mode_cache, li.key(), li);
                                } else {
                                    let err = format!("{}:{}:{}: ({})[moved data]...",
                                        key.basis, U2F_APP_DICT, key.name, key.len
                                    );
                                    self.report_err("Couldn't deserialize U2F key:", Some(err));
                                }
                            } else {
                                oom_keys += 1;
                            }
                        }
                        if oom_keys != 0 {
                            log::warn!("Ran out of cache space handling U2F tokens. {} tokens are not loaded.", oom_keys);
                            self.report_err(&format!("Ran out of cache space handling U2F entries. {} tokens not loaded", oom_keys), None::<crate::storage::Error>);
                        }
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::NotFound => {
                                // this is fine, it just means no entries have been entered yet
                            },
                            _ => {
                                log::error!("Error opening U2F dictionary");
                                self.report_err("Error opening U2F dictionary", Some(e))
                            }
                        }
                    },
                }

                {   // this brace creates a block that defines the lifetime of `mutex`; it is released on drop as it goes out of scope
                    // access to OPENSK2_DICT has to be mutex-guarded, otherwise we get errors as the
                    // OpenSK thread mutates the dictionary while we query it
                    let mutex = self.opensk_mutex.lock().unwrap();
                    log::debug!("listing in {}", OPENSK2_DICT);
                    match self.pddb.borrow().read_dict(OPENSK2_DICT, None, Some(256 * 1024)) {
                        Ok(keys) => {
                            let mut oom_keys = 0;
                            for key in keys {
                                let key_number = key.name.parse::<usize>().unwrap_or(0);
                                if vault::ctap::storage::key::CREDENTIALS.contains(&key_number) {
                                    if let Some(data) = key.data {
                                        match vault::ctap::storage::deserialize_credential(&data) {
                                            Some(result) => {
                                                let name = if let Some(display_name) = result.user_display_name {
                                                    display_name
                                                } else {
                                                    String::from_utf8(result.user_handle).unwrap_or("".to_string())
                                                };
                                                let desc = format!("{} / {} (FIDO2)", result.rp_id, String::from_utf8(result.credential_id).unwrap_or("---".to_string()));
                                                let extra = format!("{}", name);
                                                let li = ListItem {
                                                    name: desc,
                                                    extra,
                                                    dirty: true,
                                                    guid: key.name,
                                                    count: 0,
                                                    atime: 0,
                                                };
                                                self.item_lists.lock().unwrap().insert(self.mode_cache, li.key(), li);
                                            }
                                            None => {
                                                // Probably more indicative of a mismatch in OpenSK key range mapping, rather than a hard error.
                                                let err = format!("{}:{}:{}: ({}){:x?}...",
                                                    key.basis, OPENSK2_DICT, key.name, key.len, &data[..16]
                                                );
                                                log::info!("Couldn't deserialize FIDO2 key {}", err);
                                            },
                                        }
                                    } else {
                                        oom_keys += 1;
                                    }
                                }
                            }
                            if oom_keys != 0 {
                                log::warn!("Ran out of cache space handling FIDO2 tokens. {} tokens are not loaded.", oom_keys);
                                self.report_err(&format!("Ran out of cache space handling FIDO2 entries. {} tokens not loaded", oom_keys), None::<crate::storage::Error>);
                            }
                        }
                        Err(e) => {
                            match e.kind() {
                                ErrorKind::NotFound => {
                                    // this is fine, it just means no entries have been entered yet
                                },
                                _ => {
                                    log::error!("Error opening FIDO2 dictionary");
                                    self.report_err("Error opening FIDO2 dictionary", Some(e))
                                }
                            }
                        },
                    }
                    drop(mutex);
                }
            }
            VaultMode::Totp => {
                self.item_lists.lock().unwrap().clear(self.mode_cache);
                match self.pddb.borrow().read_dict(VAULT_TOTP_DICT, None, Some(256 * 1024)) {
                    Ok(keys) => {
                        let mut oom_keys = 0;
                        for key in keys {
                            if let Some(data) = key.data {
                                if let Some(totp) = storage::TotpRecord::try_from(data).ok() {
                                    let extra = format!("{}:{}:{}:{}:{}",
                                        totp.secret,
                                        totp.digits,
                                        totp.timestep,
                                        totp.algorithm,
                                        if totp.is_hotp {"HOTP"} else {"TOTP"}
                                    );
                                    let desc = format!("{}", totp.name);
                                    let li = ListItem {
                                        name: desc,
                                        extra,
                                        dirty: true,
                                        guid: key.name,
                                        count: 0,
                                        atime: 0,
                                    };
                                    self.item_lists.lock().unwrap().insert(self.mode_cache, li.key(), li);
                                } else {
                                    let err = format!("{}:{}:{}: ({})[moved data]...",
                                        key.basis, VAULT_TOTP_DICT, key.name, key.len
                                    );
                                    self.report_err("Couldn't deserialize TOTP:", Some(err));
                                }
                            } else {
                                oom_keys += 1;
                            }
                        }
                        if oom_keys != 0 {
                            log::warn!("Ran out of cache space handling FIDO2 tokens. {} tokens are not loaded.", oom_keys);
                            self.report_err(&format!("Ran out of cache space handling FIDO2 entries. {} tokens not loaded", oom_keys), None::<crate::storage::Error>);
                        }
                    }
                    Err(e) => {
                        match e.kind() {
                            ErrorKind::NotFound => {
                                // this is fine, it just means no entries have been entered yet
                            },
                            _ => {
                                log::error!("Error opening FIDO2 dictionary");
                                self.report_err("Error opening FIDO2 dictionary", Some(e))
                            }
                        }
                    },
                }
            }
        }
        log::debug!("heap usage B: {}", heap_usage());
        #[cfg(feature="vaultperf")]
        {
            self.perfentry(&self.pm, PERFMETA_ENDBLOCK, 0, std::line!());
            self.pm.flush().ok();
            match self.pm.stop_and_flush() {
                Ok(entries) => {
                    log::info!("entries: {}", entries);
                }
                _ => {
                    log::info!("Perfcounter OOM'd during run");
                }
            }
            log::info!("Buf vmem loc: {:x}", self.perfbuf.as_ptr() as u32);
            log::info!("Buf pmem loc: {:x}", xous::syscall::virt_to_phys(self.perfbuf.as_ptr() as usize).unwrap_or(0));
            log::info!("PerfLogEntry size: {}", core::mem::size_of::<PerfLogEntry>());
            log::info!("Now printing the page table mapping for the performance buffer:");
            for page in (0..BUFLEN).step_by(4096) {
                log::info!("V|P {:x} {:x}",
                    self.perfbuf.as_ptr() as usize + page,
                    xous::syscall::virt_to_phys(self.perfbuf.as_ptr() as usize + page).unwrap_or(0),
                );
            }
        }
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
        #[cfg(feature="ux-swap-delay")]
        self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
        match self.pddb.borrow().unlock_basis(&name, Some(BasisRetentionPolicy::Persist)) {
            Ok(_) => {
                log::debug!("Basis {} unlocked", name);
                // clear local caches
                self.item_lists.lock().unwrap().clear_all();
            },
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
                            Ok(_) => {
                                log::debug!("basis {} locked", b);
                                // clear local caches
                                self.item_lists.lock().unwrap().clear_all();
                            },
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
            #[cfg(feature="ux-swap-delay")]
            self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
            match self.pddb.borrow().create_basis(&name) {
                Ok(_) => {
                    if self.yes_no_approval(t!("vault.basis.created_mount", xous::LANG)) {
                        match self.pddb.borrow().unlock_basis(&name, Some(BasisRetentionPolicy::Persist)) {
                            Ok(_) => {
                                log::debug!("Basis {} unlocked", name);
                                // clear local caches
                                self.item_lists.lock().unwrap().clear_all();
                            },
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
                Err(e) => {
                    match e.kind() {
                        ErrorKind::AlreadyExists => {
                            self.modals.show_notification(t!("vault.basis.already_exists", xous::LANG), None).ok();
                        }
                        _ => {
                            self.report_err(t!("vault.error.internal_error", xous::LANG), Some(e))
                        }
                    }
                },
            }
        } else {
                // do nothing
            }
        }
    }

    #[cfg(feature="vault-testing")]
    pub(crate) fn populate_tests(&mut self) {
        self.modals.dynamic_notification(Some("Creating test entries..."), None).ok();
        let words = [
            "bunnie", "foo", "turtle.net", "Fox.ng", "Bear", "dog food", "Cat.com", "FUzzy", "1off", "www_test_site_com/long_name/stupid/foo.htm",
            "._weird~yy%\":'test", "//WHYwhyWHY", "Xyz|zy", "foo:bar", "f🍕🍔🍟🌭d", "💎🙌", "some ノート", "笔录4u", "@u", "sane text", "Käsesoßenrührlöffel",
            "entropy", "👀", "mysite.com", "hax", "1336", "yo", "b", "mando", "Grogu", "zebra", "aws"];
        let weights = [1; 21];
        const TARGET_ENTRIES: usize = 12;
        const TARGET_ENTRIES_PW: usize = 350;
        // for each database, populate up to TARGET_ENTRIES
        // as this is testing code, it's written a bit more fragile in terms of error handling (fail-panic, rather than fail-dialog)
        // --- passwords ---
        // TODO(gsora): we gotta figure out how to remove the pddb dep when testing feature is not
        // enabled
        let pws = self.pddb.borrow().list_keys(VAULT_PASSWORD_DICT, None).unwrap_or(Vec::new());
        if pws.len() < TARGET_ENTRIES_PW {
            let extra_count = TARGET_ENTRIES_PW - pws.len();
            for _index in 0..extra_count {
                let desc = random_pick::pick_multiple_from_slice(&words, &weights, 3);
                // this exposes raw unicode and symbols to the sorting list
                // let description = format!("{} {} {}", desc[0], desc[1], desc[2]);
                // this will make a list that's a bit more challenging for the sorter to deal with because it has more bins
                let r = self.trng.borrow_mut().get_u32().unwrap();
                let description = format!("{}{}{} {} {}",
                    char::from_u32((r % 26) + 0x61).unwrap_or('.'),
                    char::from_u32(((r >> 8) % 26) + 0x61).unwrap_or('.'),
                    desc[0], desc[1], desc[2]);
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
                let mut record = storage::PasswordRecord {
                    version: VAULT_PASSWORD_REC_VERSION,
                    description,
                    username,
                    password,
                    notes,
                    ctime: 0,
                    atime: 0,
                    count: 0,
                };

                match self.storage.borrow_mut().new_record(&mut record, None, true) {
                    Ok(_) => {},
                    Err(e) => log::error!("PW Error: {:?}", e),
                };

            }
        }
        // --- U2F + FIDO ---
        let fido = self.pddb.borrow().list_keys(OPENSK2_DICT, None).unwrap_or(Vec::new());
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
                self.trng.borrow_mut().fill_bytes_via_next(&mut id);
                let record = vault::AppInfo {
                    name,
                    id,
                    notes,
                    ctime: 1, // zero ctime is disallowed
                    atime: 1,
                    count: 0,
                };
                let ser = serialize_app_info(&record);
                let app_id_str = hex::encode(id);
                match self.pddb.borrow().get(
                    U2F_APP_DICT,
                    &app_id_str,
                    None, true, true,
                    Some(256), Some(basis_change)
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
                use vault::ctap::data_formats::*;
                use ctap_crypto::rng256::Rng256;
                let _c_id = random_pick::pick_multiple_from_slice(&words, &weights, 2);
                let cred_id = format!("{}", index + 1800); // 1800 is extracted from ctap/storage/keys.rs; 1700 is the start of the credential region, this sticks it...somewhere "above" that
                let r_id = random_pick::pick_multiple_from_slice(&words, &weights, 2);
                let rp_id = format!("{} {}", r_id[0], r_id[1]);
                let handle = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let new_credential = PublicKeyCredentialSource {
                    key_type: PublicKeyCredentialType::PublicKey,
                    credential_id: rng.gen_uniform_u8x32().to_vec(),
                    private_key: vault::ctap::crypto_wrapper::PrivateKey::Ecdsa(rng.gen_uniform_u8x32()),
                    rp_id,
                    user_handle: handle.as_bytes().to_vec(),
                    user_display_name: None,
                    cred_protect_policy: None,
                    creation_order: 0,
                    user_name: None,
                    user_icon: None,
                    cred_blob: None,
                    large_blob_key: None,
                };
                let shortid = &cred_id;
                match self.pddb.borrow().get(
                    OPENSK2_DICT,
                    shortid,
                    None, true, true,
                    Some(128), Some(basis_change)
                ) {
                    Ok(mut cred) => {
                        let value = vault::ctap::storage::serialize_credential(new_credential).unwrap();
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
            for _index in 0..extra {
                let names = random_pick::pick_multiple_from_slice(&words, &weights, 3);
                let name = format!("{} {} {}", names[0], names[1], names[2]);
                let notes = random_pick::pick_from_slice(&words, &weights).unwrap().to_string();
                let mut secret_bytes = [0u8; 10];
                self.trng.borrow_mut().fill_bytes_via_next(&mut secret_bytes);
                let mut record = storage::TotpRecord {
                    version: VAULT_TOTP_REC_VERSION,
                    secret: base32::encode(base32::Alphabet::RFC4648 { padding: false }, &secret_bytes),
                    name,
                    algorithm: TotpAlgorithm::HmacSha1,
                    notes,
                    digits: 6,
                    timestep: 30,
                    ctime: 0,
                    is_hotp: false,
                };

                match self.storage.borrow_mut().new_record(&mut record, None, true) {
                    Ok(_) => {},
                    Err(e) => log::error!("PW Error: {:?}", e),
                };
            }
            // specific TOTP entry with a known shared secret for testing
            let mut record = storage::TotpRecord {
                version: VAULT_TOTP_REC_VERSION,
                secret: "I65VU7K5ZQL7WB4E".to_string(),
                name: "totp@authenticationtest.com".to_string(),
                algorithm: TotpAlgorithm::HmacSha1,
                notes: "Predefined test".to_string(),
                digits: 6,
                timestep: 30,
                ctime: 0,
                is_hotp: false,
            };

            match self.storage.borrow_mut().new_record(&mut record, None, true) {
                Ok(_) => {},
                Err(e) => log::error!("PW Error: {:?}", e),
            };

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
fn count_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
    let text_str = input.as_str();
    match text_str.parse::<u64>() {
        Ok(_input_int) => None,
        _ => Some(xous_ipc::String::<256>::from_str(t!("vault.illegal_count", xous::LANG))),
    }
}

#[cfg(feature="vaultperf")]
fn build_perf_mgr<'a>(bufptr: *mut u8) -> PerfMgr<'a> {
    use utralib::generated::*;
    let perf_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::perfcounter::HW_PERFCOUNTER_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map perfcounter CSR range: check that no other performance managers are active");
    // this is the range used by the shellchat performance manager
    let event1_csr = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::event_source1::HW_EVENT_SOURCE1_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map event1 CSR range");
    PerfMgr::new(
        bufptr,
        AtomicCsr::new(perf_csr.as_mut_ptr() as *mut u32),
        AtomicCsr::new(event1_csr.as_mut_ptr() as *mut u32) // event_source1
    )
}

pub(crate) fn heap_usage() -> usize {
    match xous::rsyscall(xous::SysCall::IncreaseHeap(0, xous::MemoryFlags::R)).expect("couldn't get heap size") {
        xous::Result::MemoryRange(m) => {
            let usage = m.len();
            usage
        }
        _ => {
            log::error!("Couldn't measure heap usage");
            0
         },
    }
}

fn make_pw_name(description: &str, username: &str, dest: &mut String) {
    dest.clear();
    dest.push_str(description);
    dest.push_str("/");
    dest.push_str(username);
}