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
const ROOM_KEY: &str = "room";
const SINCE_KEY: &str = "_since";
const SERVER_KEY: &str = "server";
const TOKEN_KEY: &str = "_token";
const USER_KEY: &str = "user";
const USERNAME_KEY: &str = "username";

const HTTPS: &str = "https://";
const SERVER_MATRIX: &str = "https://matrix.org";

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
    username: String,
    server: String,
    token: String,
    logged_in: bool,
    room_id: String,
    filter: String,
    since: String,
}
impl MtxChat {
    pub fn new() -> MtxChat {
        let xns = xous_names::XousNames::new().unwrap();
        let common = MtxChat {
            user: EMPTY.to_string(),
            username: EMPTY.to_string(),
            server: SERVER_MATRIX.to_string(),
            token: EMPTY.to_string(),
            logged_in: false,
            room_id: EMPTY.to_string(),
            filter: EMPTY.to_string(),
            since: EMPTY.to_string(),
        };
        let mut keypath = PathBuf::new();
        keypath.push(MTXCHAT_DICT);
        if std::fs::metadata(&keypath).is_ok() { // keypath exists
            // log::info!("dict '{}' exists", MTXCHAT_DICT);
        } else {
            log::info!("dict '{}' does NOT exist.. creating it", MTXCHAT_DICT);
            match std::fs::create_dir_all(&keypath){
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
                ROOM_KEY => {
                    self.set_room();
                }
                SERVER_KEY => {
                    self.server = value.to_string();
                }
                SINCE_KEY => {
                    self.since = value.to_string();
                }
                USERNAME_KEY => {
                    self.username = value.to_string();
                }
                USER_KEY => {
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
        log::info!("# USER_KEY set '{}' = '{}'", USER_KEY, value);
        let i = match value.find('@') {
            Some(index) => index + 1,
            None => 0,
        };
        let j = match value.find(':') {
            Some(index) => index,
            None => value.len(),
        };
        self.username = (&value[i..j]).to_string();
        if j < value.len() {
            self.server = String::from(HTTPS);
            self.server.push_str(&value[j + 1..]);
        } else {
            self.server = SERVER_MATRIX.to_string();
        }
        self.user = value.to_string();
        log::info!(
            "# user = '{}' username = '{}' server = '{}'",
            self.user,
            self.username,
            self.server
        );
        self.set_debug(USERNAME_KEY, &self.username.clone());
        self.set_debug(SERVER_KEY, &self.server.clone());
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
            "# ROOM_KEY set '{}' => clearing ROOM_ID_KEY, SINCE_KEY, FILTER_KEY",
            ROOM_KEY
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
                SINCE_KEY => {
                    self.since = EMPTY.to_string();
                }
                SERVER_KEY => {
                    self.server = EMPTY.to_string();
                }
                USER_KEY => {
                    self.user = EMPTY.to_string();
                }
                USERNAME_KEY => {
                    self.username = EMPTY.to_string();
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
        // self.scalar_async_msg(LOGGING_IN_ID);
        self.token = self.get_default(TOKEN_KEY, EMPTY);
        self.logged_in = false;
        if self.token.len() > 0 {
            if web::whoami(&self.server, &self.token) {
                self.logged_in = true;
            }
        }
        if !self.logged_in {
            if web::get_login_type(&self.server) {
                let user = self.get_default(USER_KEY, USER_KEY);
                if user.len() == 0 {
                    // self.scalar_async_msg(SET_USER_ID);
                    self.modals
                        .show_notification(t!("mtxcli.please.set.user", locales::LANG), None)
                        .expect("notification failed");
                }
                let password = self.get_default(PASSWORD_KEY, EMPTY);
                if password.len() == 0 {
                    // self.scalar_async_msg(SET_PASSWORD_ID);
                } else {
                    if let Some(new_token) = web::authenticate_user(&self.server, &user, &password)
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
            // self.scalar_async_msg(LOGGED_IN_ID);
            log::info!("logged_in");
        } else {
            // self.scalar_async_msg(LOGIN_FAILED_ID);
            log::info!("login failed");
        }
        self.logged_in
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
