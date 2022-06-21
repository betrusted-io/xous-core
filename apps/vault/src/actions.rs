use std::thread;
use gam::TextEntryPayload;
use std::sync::{Arc, Mutex};
use num_traits::*;
use xous::{SID, msg_blocking_scalar_unpack};
use locales::t;
use std::io::{Write, Read};
use passwords::PasswordGenerator;
use chrono::{Utc, DateTime, NaiveDateTime};
use std::time::{SystemTime, UNIX_EPOCH};
use std::cell::RefCell;

use crate::ux::ListItem;
use crate::VaultMode;

const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
const VAULT_PASSWORD_REC_VERSION: u32 = 1;
/// time allowed between dialog box swaps for background operations to redraw
const SWAP_DELAY_MS: usize = 150;

struct PasswordRecord {
    description: String,
    username: String,
    password: String,
    ctime: u64,
    atime: u64,
    count: u64,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum ActionOp {
    /// Menu items
    MenuAutotype,
    MenuAddnew,
    MenuEdit,
    MenuDelete,
    /// Internal ops
    UpdateMode,
    Quit,
}

pub(crate) fn start_actions_thread(sid: SID, mode: Arc::<Mutex::<VaultMode>>, item_list: Arc::<Mutex::<Vec::<ListItem>>>) {
    let _ = thread::spawn({
        move || {
            let mut manager = ActionManager::new(mode, item_list);
            loop {
                let msg = xous::receive_message(sid).unwrap();
                log::trace!("got message {:?}", msg);
                match FromPrimitive::from_usize(msg.body.id()) {
                    Some(ActionOp::MenuAddnew) => {
                        manager.menu_addnew();
                    },
                    Some(ActionOp::MenuAutotype) => {

                    },
                    Some(ActionOp::MenuDelete) => {

                    },
                    Some(ActionOp::MenuEdit) => {

                    },
                    Some(ActionOp::UpdateMode) => msg_blocking_scalar_unpack!(msg, _, _, _, _,{
                        manager.gen_fake_data();
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
}
impl ActionManager {
    pub fn new(mode: Arc::<Mutex::<VaultMode>>, item_list: Arc::<Mutex::<Vec::<ListItem>>>) -> ActionManager {
        let xns = xous_names::XousNames::new().unwrap();
        ActionManager {
            modals: modals::Modals::new(&xns).unwrap(),
            trng: RefCell::new(trng::Trng::new(&xns).unwrap()),
            mode,
            item_list,
            pddb: RefCell::new(pddb::Pddb::new()),
            tt: ticktimer_server::Ticktimer::new().unwrap(),
        }
    }
    pub(crate) fn menu_addnew(&mut self) {
        match *self.mode.lock().unwrap() {
            VaultMode::Password => {
                let description = match self.modals
                    .alert_builder(t!("vault.newitem.name", xous::LANG))
                    .field(None, Some(name_validator))
                    .build()
                {
                    Ok(text) => {
                        text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string()
                    },
                    _ => {log::error!("Name entry failed"); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let username = match self.modals
                    .alert_builder(t!("vault.newitem.username", xous::LANG))
                    .field(None, Some(name_validator))
                    .build()
                {
                    Ok(text) => text.content()[0].content.as_str().unwrap_or("UTF-8 error").to_string(),
                    _ => {log::error!("Name entry failed"); return}
                };
                self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                let mut approved = false;
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
                        _ => {log::error!("Name entry failed"); return}
                    };
                    self.tt.sleep_ms(SWAP_DELAY_MS).unwrap();
                    password = if maybe_password.len() == 0 {
                        let length = match self.modals
                            .alert_builder(t!("vault.newitem.configure_length", xous::LANG))
                            .field(Some("20".to_string()), Some(length_validator))
                            .build()
                        {
                            Ok(entry) => entry.content()[0].content.as_str().unwrap().parse::<u32>().unwrap(),
                            _ => {log::error!("Length entry failed"); return}
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
                            _ => {log::error!("Modal selection error"); return}
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
                    description,
                    username,
                    password,
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
                    Some(256), Some(crate::basis_change)
                ) {
                    Ok(mut data) => {
                        data.write(&ser).expect("couldn't store password record");
                    }
                    _ => log::error!("Error storing new password"),
                }
                self.pddb.borrow().sync().ok();
            }
            _ => {} // not valid for these other modes
        }
    }
    #[allow(dead_code)]
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

    // populates the display list with testing data
    pub(crate) fn gen_fake_data(&mut self) {
        let il = &mut *self.item_list.lock().unwrap();
        il.clear();
        match *self.mode.lock().unwrap() {
            VaultMode::Fido | VaultMode::Password => {
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


fn name_validator(input: TextEntryPayload) -> Option<xous_ipc::String<256>> {
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

fn serialize_password<'a>(record: &PasswordRecord) -> Vec::<u8> {
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}",
        "version", VAULT_PASSWORD_REC_VERSION,
        "description", record.description,
        "username", record.username,
        "password", record.password,
        "ctime", record.ctime,
        "atime", record.atime,
        "count", record.count,
    ).into_bytes()
}

fn deserialize_password(data: Vec::<u8>) -> Option<PasswordRecord> {
    None
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid target
fn utc_now() -> DateTime::<Utc> {
    let now =
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}
