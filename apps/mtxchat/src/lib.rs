pub mod api;
mod url;
mod web;

pub use api::*;

use locales::t;
use modals::Modals;

use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write as StdWrite};
use std::path::PathBuf;

/// PDDB Dict for mtxchat keys
const MTXCHAT_DICT: &str = "mtxchat";

const FILTER_KEY: &str = "_filter";
const PASSWORD_KEY: &str = "password";
const ROOM_ID_KEY: &str = "_room_id";
const ROOM_NAME_KEY: &str = "room_name";
const ROOM_SERVER_KEY: &str = "room_server";
const SINCE_KEY: &str = "_since";
const TOKEN_KEY: &str = "_token";
const USER_ID_KEY: &str = "_user_id";
const USER_NAME_KEY: &str = "user_name";
const USER_SERVER_KEY: &str = "user_server";

const HTTPS: &str = "https://";
const SERVER_MATRIX: &str = "matrix.org";

const EMPTY: &str = "";

pub const CLOCK_NOT_SET_ID: usize = 1;
pub const PDDB_NOT_MOUNTED_ID: usize = 2;
pub const WIFI_NOT_CONNECTED_ID: usize = 3;
pub const MTXCLI_INITIALIZED_ID: usize = 4;
pub const WIFI_CONNECTED_ID: usize = 5;
pub const SET_USER_ID: usize = 6;
pub const SET_PASSWORD_ID: usize = 7;
pub const LOGGED_IN_ID: usize = 8;
pub const LOGIN_FAILED_ID: usize = 9;
pub const SET_ROOM_ID: usize = 10;
pub const ROOMID_FAILED_ID: usize = 11;
pub const FILTER_FAILED_ID: usize = 12;
pub const SET_SERVER_ID: usize = 13;
pub const LOGGING_IN_ID: usize = 14;
pub const LOGGED_OUT_ID: usize = 15;
pub const NOT_CONNECTED_ID: usize = 16;
pub const FAILED_TO_SEND_ID: usize = 17;
pub const PLEASE_LOGIN_ID: usize = 18;

#[cfg(not(target_os = "xous"))]
pub const HOSTED_MODE: bool = true;
#[cfg(target_os = "xous")]
pub const HOSTED_MODE: bool = false;

//#[derive(Debug)]
pub struct MtxChat {
    user: String,
    user_id: String,
    user_name: String,
    user_server: String,
    token: String,
    logged_in: bool,
    room_id: String,
    room_server: String,
    filter: String,
    since: String,
    modals: Modals,
}
impl MtxChat {
    pub fn new() -> MtxChat {
        let xns = xous_names::XousNames::new().unwrap();
        let modals = Modals::new(&xns).expect("can't connect to Modals server");
        let common = MtxChat {
            user: EMPTY.to_string(),
            user_id: EMPTY.to_string(),
            user_name: EMPTY.to_string(),
            user_server: SERVER_MATRIX.to_string(),
            token: EMPTY.to_string(),
            logged_in: false,
            room_id: EMPTY.to_string(),
            room_server: EMPTY.to_string(),
            filter: EMPTY.to_string(),
            since: EMPTY.to_string(),
            modals: modals,
        };
        let mut keypath = PathBuf::new();
        keypath.push(MTXCHAT_DICT);
        if std::fs::metadata(&keypath).is_ok() { // keypath exists
             // log::info!("dict '{}' exists", MTXCHAT_DICT);
        } else {
            log::info!("dict '{}' does NOT exist.. creating it", MTXCHAT_DICT);
            match std::fs::create_dir_all(&keypath) {
                Ok(_) => log::info!("created dict: {}", MTXCHAT_DICT),
                Err(e) => log::warn!("failed to create dict: {:?}", e),
            }
        }
        common
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "may not set a variable beginning with __ ",
            ))
        } else {
            log::info!("set '{}' = '{}'", key, value);
            let mut keypath = PathBuf::new();
            keypath.push(MTXCHAT_DICT);
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                 // log::info!("dict '{}' exists", MTXCHAT_DICT);
            } else {
                log::info!("dict '{}' does NOT exist.. creating it", MTXCHAT_DICT);
                std::fs::create_dir_all(&keypath)?;
            }
            keypath.push(key);
            File::create(keypath)?.write_all(value.as_bytes())?;
            match key {
                // special case side effects
                FILTER_KEY => {
                    self.filter = value.to_string();
                }
                PASSWORD_KEY => {
                    self.set_password();
                }
                ROOM_ID_KEY => {
                    self.room_id = value.to_string();
                }
                ROOM_NAME_KEY => {
                    self.set_room();
                }
                ROOM_SERVER_KEY => {
                    self.room_server = value.to_string();
                }
                SINCE_KEY => {
                    self.since = value.to_string();
                }
                USER_NAME_KEY => {
                    self.user_name = value.to_string();
                }
                USER_SERVER_KEY => {
                    self.user_server = value.to_string();
                }
                USER_ID_KEY => {
                    self.set_user(value);
                }
                _ => {}
            }
            Ok(())
        }
    }

    // will log on error (vs. panic)
    pub fn set_debug(&mut self, key: &str, value: &str) -> bool {
        match self.set(key, value) {
            Ok(()) => true,
            Err(e) => {
                log::info!("error setting key {}: {:?}", key, e);
                false
            }
        }
    }

    pub fn set_user(&mut self, value: &str) {
        log::info!("# USER_ID_KEY set '{}' = '{}'", USER_ID_KEY, value);
        let i = match value.find('@') {
            Some(index) => index + 1,
            None => 0,
        };
        let j = match value.find(':') {
            Some(index) => index,
            None => value.len(),
        };
        self.user_name = (&value[i..j]).to_string();
        if j < value.len() {
            self.user_server = String::from(HTTPS);
            self.user_server.push_str(&value[j + 1..]);
        } else {
            self.user_server = SERVER_MATRIX.to_string();
        }
        self.user_id = value.to_string();
        log::info!(
            "# user = '{}' user_name = '{}' server = '{}'",
            self.user_id,
            self.user_name,
            self.user_server
        );
        self.set_debug(USER_NAME_KEY, &self.user_name.clone());
        self.set_debug(USER_SERVER_KEY, &self.user_server.clone());
        self.unset_debug(TOKEN_KEY);
    }

    pub fn set_password(&mut self) {
        log::info!(
            "# PASSWORD_KEY set '{}' => clearing TOKEN_KEY",
            PASSWORD_KEY
        );
        self.unset_debug(TOKEN_KEY);
    }

    pub fn set_room(&mut self) {
        log::info!(
            "# ROOM_NAME_KEY set '{}' => clearing ROOM_ID_KEY, SINCE_KEY, FILTER_KEY",
            ROOM_NAME_KEY
        );
        self.unset_debug(ROOM_ID_KEY);
        self.unset_debug(SINCE_KEY);
        self.unset_debug(FILTER_KEY);
    }

    pub fn unset(&mut self, key: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "may not unset a variable beginning with __ ",
            ))
        } else {
            log::info!("unset '{}'", key);
            let mut keypath = PathBuf::new();
            keypath.push(MTXCHAT_DICT);
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                 // log::info!("dict '{}' exists", MTXCHAT_DICT);
            } else {
                log::info!("dict '{}' does NOT exist.. creating it", MTXCHAT_DICT);
                std::fs::create_dir_all(&keypath)?;
            }
            keypath.push(key);
            if std::fs::metadata(&keypath).is_ok() {
                // keypath exists
                log::info!("dict:key = '{}:{}' exists.. deleting it", MTXCHAT_DICT, key);
                std::fs::remove_file(keypath)?;
            }
            match key {
                // special case side effects -- update cached values
                FILTER_KEY => {
                    self.filter = EMPTY.to_string();
                }
                ROOM_ID_KEY => {
                    self.room_id = EMPTY.to_string();
                }
                ROOM_SERVER_KEY => {
                    self.room_server = EMPTY.to_string();
                }
                SINCE_KEY => {
                    self.since = EMPTY.to_string();
                }
                USER_SERVER_KEY => {
                    self.user_server = EMPTY.to_string();
                }
                USER_ID_KEY => {
                    self.user_id = EMPTY.to_string();
                }
                USER_NAME_KEY => {
                    self.user_name = EMPTY.to_string();
                }
                _ => {}
            }
            Ok(())
        }
    }

    // will log on error (vs. panic)
    pub fn unset_debug(&mut self, key: &str) -> bool {
        match self.unset(key) {
            Ok(()) => true,
            Err(e) => {
                log::info!("error unsetting key {}: {:?}", key, e);
                false
            }
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<String>, Error> {
        // if key.eq(CURRENT_VERSION_KEY) {
        //     Ok(Some(self.version.clone()))
        // } else {
        let mut keypath = PathBuf::new();
        keypath.push(MTXCHAT_DICT);
        keypath.push(key);
        if let Ok(mut file) = File::open(keypath) {
            let mut value = String::new();
            file.read_to_string(&mut value)?;
            log::info!("get '{}' = '{}'", key, value);
            Ok(Some(value))
        } else {
            Ok(None)
        }
        // }
    }

    pub fn get_default(&mut self, key: &str, default: &str) -> String {
        match self.get(key) {
            Ok(None) => default.to_string(),
            Ok(Some(value)) => value.to_string(),
            Err(e) => {
                log::info!("error getting key {}: {:?}", key, e);
                default.to_string()
            }
        }
    }

    pub fn login(&mut self) -> bool {
        self.token = self.get_default(TOKEN_KEY, EMPTY);
        self.logged_in = false;
        if self.token.len() > 0 {
            if web::whoami(&self.user_server, &self.token) {
                self.logged_in = true;
            }
        }
        if !self.logged_in {
            if web::get_login_type(&self.user_server) {
                let user = self.get_default(USER_ID_KEY, USER_ID_KEY);
                if user.len() == 0 {}
                let password = self.get_default(PASSWORD_KEY, EMPTY);
                if password.len() == 0 {
                } else {
                    if let Some(new_token) = web::authenticate_user(&self.user_server, &user, &password)
                    {
                        self.set_debug(TOKEN_KEY, &new_token);
                        self.logged_in = true;
                    } else {
                        log::info!(
                            "Error: cannnot login with type: {}",
                            web::MTX_LOGIN_PASSWORD
                        );
                    }
                }
            }
        }
        if self.logged_in {
            log::info!("logged_in");
        } else {
            log::info!("login failed");
        }
        self.logged_in
    }

    pub fn login_modal(&mut self) {
        const HIDE: &str = "*****";
        let mut builder = self.modals.alert_builder(t!("mtxchat.login.title", locales::LANG));
        let builder = match self.get(USER_SERVER_KEY) {
            Ok(Some(server)) => builder.field_placeholder_persist(Some(server), None),
            _ => builder.field(Some(t!("mtxchat.server", locales::LANG).to_string()), None),
        };
        let builder = match self.get(USER_NAME_KEY) {
            Ok(Some(user)) => builder.field_placeholder_persist(Some(user), None),
            _ => builder.field(Some(t!("mtxchat.user_name", locales::LANG).to_string()), None),
        };
        let builder = match self.get(PASSWORD_KEY) {
            Ok(Some(pwd)) => builder.field_placeholder_persist(Some(HIDE.to_string()), None),
            _ => builder.field(Some(t!("mtxchat.password", locales::LANG).to_string()), None),
        };
        if let Ok(payloads) = builder.build() {
            let mut user = "@".to_string();
            if let Ok(content) = payloads.content()[1].content.as_str() {
                self.set(USER_NAME_KEY, content)
                    .expect("failed to save username");
                user.push_str(content);
            }
            user.push_str(":");
            if let Ok(content) = payloads.content()[0].content.as_str() {
                self.set(USER_SERVER_KEY, content)
                    .expect("failed to save server");
                    user.push_str(content);
            }
            self.set(USER_ID_KEY, &user).expect("failed to save user");
            self.user_id = user;
            if let Ok(content) = payloads.content()[2].content.as_str() {
                if content.ne(HIDE) {
                    self.set(PASSWORD_KEY, content)
                        .expect("failed to save password");
                }
            }
        }
    }

    // assume logged in, token is valid
    pub fn get_room_id(&mut self) -> bool {
        if self.room_id.len() > 0 {
            true
        } else {
            let room = self.get_default(ROOM_NAME_KEY, EMPTY);
            let server = self.get_default(ROOM_SERVER_KEY, EMPTY);
            if room.len() == 0 {
                false
            } else if server.len() == 0 {
                false
            } else {
                let mut room_server = String::new();
                if ! room.starts_with("#") {
                    room_server.push_str("#");
                }
                room_server.push_str(&room);
                room_server.push_str(":");
                let i = match server.find(HTTPS) {
                    Some(index) => {
                        index + HTTPS.len()
                    },
                    None => {
                        server.len()
                    },
                };
                if i >= server.len() {
                    false
                } else {
                    room_server.push_str(&server[i..]);
                    if let Some(new_room_id) = web::get_room_id(&self.room_server, &room_server, &self.token) {
                        self.set_debug(ROOM_ID_KEY, &new_room_id);
                        true
                    } else {
                        false
                    }
                }
            }
        }
    }

    pub fn room_modal(&mut self){
        let mut builder = self.modals.alert_builder(t!("mtxchat.room.title", locales::LANG));
        let builder = match self.get(ROOM_NAME_KEY) {
            Ok(Some(room)) => builder.field_placeholder_persist(Some(room), None),
            _ => builder.field(Some(t!("mtxchat.room.name", locales::LANG).to_string()), None),
        };
        let builder = match self.get(ROOM_SERVER_KEY) {
            Ok(Some(server)) => builder.field_placeholder_persist(Some(server), None),
            _ => builder.field(Some(t!("mtxchat.server", locales::LANG).to_string()), None),
        };
        if let Ok(payloads) = builder.build() {
            let mut room_id = "#".to_string();
            if let Ok(content) = payloads.content()[0].content.as_str() {
                self.set(ROOM_NAME_KEY, content)
                    .expect("failed to save server");
                    room_id.push_str(content);
            }
            room_id.push_str(":");
            if let Ok(content) = payloads.content()[1].content.as_str() {
                self.set(ROOM_SERVER_KEY, content)
                    .expect("failed to save server");
                    room_id.push_str(content);

            }
            self.set(ROOM_ID_KEY, &room_id).expect("failed to save server");
        }
    }

            }
        }
    }
}

pub(crate) fn heap_usage() -> usize {
    match xous::rsyscall(xous::SysCall::IncreaseHeap(0, xous::MemoryFlags::R))
        .expect("couldn't get heap size")
    {
        xous::Result::MemoryRange(m) => {
            let usage = m.len();
            usage
        }
        _ => {
            log::info!("Couldn't measure heap usage");
            0
        }
    }
}
