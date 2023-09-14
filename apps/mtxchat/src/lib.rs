pub mod api;
mod listen;
mod url;
mod web;

use crate::web::get_username;
pub use api::*;
use chat::Chat;
use listen::listen;
use locales::t;
use modals::Modals;
use pddb::Pddb;
use std::fmt::Write as _;
use std::io::{Error, ErrorKind, Read, Write as StdWrite};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tls::Tls;
use trng::*;
use ureq::Agent;

/// PDDB Dict for mtxchat keys
const MTXCHAT_STATE: &str = "mtxchat.state";
const MTXCHAT_DIALOGUE: &str = "mtxchat.dialogue";

const FILTER_KEY: &str = "_filter";
const PASSWORD_KEY: &str = "password";
const ROOM_ID_KEY: &str = "_room_id";
const ROOM_NAME_KEY: &str = "room_name";
const ROOM_DOMAIN_KEY: &str = "room_domain";
const SINCE_KEY: &str = "_since";
const TOKEN_KEY: &str = "_token";
const USER_ID_KEY: &str = "_user_id";
const USER_NAME_KEY: &str = "user_name";
const USER_DOMAIN_KEY: &str = "user_domain";

const HTTPS: &str = "https://";
const DOMAIN_MATRIX: &str = "matrix.org";

const MTX_LONG_TIMEOUT: i32 = 60000; // ms
const WIFI_TIMEOUT: u32 = 10; // seconds

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
pub struct MtxChat<'a> {
    chat: &'a Chat,
    trng: Trng,
    pddb: Pddb,
    netmgr: net::NetManager,
    user_id: Option<String>,
    user_name: Option<String>,
    user_domain: Option<String>,
    agent: Agent,
    token: Option<String>,
    logged_in: bool,
    room_id: Option<String>,
    room_name: Option<String>,
    room_domain: Option<String>,
    filter: Option<String>,
    since: Option<String>,
    listening: bool,
    modals: Modals,
    new_username: bool,
    new_room: bool,
}
impl<'a> MtxChat<'a> {
    pub fn new(chat: &Chat) -> MtxChat {
        let xns = xous_names::XousNames::new().unwrap();
        let modals = Modals::new(&xns).expect("can't connect to Modals server");
        let trng = Trng::new(&xns).unwrap();
        let pddb = pddb::Pddb::new();
        pddb.try_mount();
        let tls = Tls::new();
        MtxChat {
            chat: chat,
            trng: trng,
            pddb: pddb,
            netmgr: net::NetManager::new(),
            user_id: None,
            user_name: None,
            user_domain: Some(DOMAIN_MATRIX.to_string()),
            agent: ureq::builder()
                .tls_config(Arc::new(tls.client_config()))
                .build(),
            token: None,
            logged_in: false,
            room_id: None,
            room_name: None,
            room_domain: None,
            filter: None,
            since: None,
            listening: false,
            modals: modals,
            new_username: false,
            new_room: false,
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "may not set a variable beginning with __ ",
            ))
        } else {
            log::info!("set '{}' = '{}'", key, value);
            // delete key first to ensure data in a prior longer key is gone
            self.pddb.delete_key(MTXCHAT_STATE, key, None).ok();
            match self
                .pddb
                .get(MTXCHAT_STATE, key, None, true, true, None, None::<fn()>)
            {
                Ok(mut pddb_key) => match pddb_key.write(&value.as_bytes()) {
                    Ok(len) => {
                        self.pddb.sync().ok();
                        log::trace!("Wrote {} bytes to {}:{}", len, MTXCHAT_STATE, key);
                    }
                    Err(e) => {
                        log::warn!("Error writing {}:{} {:?}", MTXCHAT_STATE, key, e);
                    }
                },
                Err(e) => log::warn!("failed to set pddb {}:{}  {:?}", MTXCHAT_STATE, key, e),
            };
            match key {
                // update cached values
                FILTER_KEY => self.filter = Some(value.to_string()),
                PASSWORD_KEY => (),
                ROOM_ID_KEY => self.room_id = Some(value.to_string()),
                ROOM_NAME_KEY => self.room_name = Some(value.to_string()),
                ROOM_DOMAIN_KEY => self.room_domain = Some(value.to_string()),
                SINCE_KEY => self.since = Some(value.to_string()),
                TOKEN_KEY => self.token = Some(value.to_string()),
                USER_NAME_KEY => self.user_name = Some(value.to_string()),
                USER_DOMAIN_KEY => self.user_domain = Some(value.to_string()),
                USER_ID_KEY => self.user_id = Some(value.to_string()),
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

    pub fn unset(&mut self, key: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(
                ErrorKind::PermissionDenied,
                "may not unset a variable beginning with __ ",
            ))
        } else {
            log::info!("unset '{}'", key);
            match self.pddb.delete_key(MTXCHAT_STATE, key, None) {
                Ok(_) => log::info!("pddb key deleted: {key}"),
                Err(e) => match e.kind() {
                    ErrorKind::NotFound => (), // ignore, nothing to do
                    _ => log::warn!("failed to delete pddb key: {key}: {:?}", e),
                },
            }
            match key {
                // update cached values
                FILTER_KEY => self.filter = None,
                PASSWORD_KEY => (),
                ROOM_ID_KEY => self.room_id = None,
                ROOM_DOMAIN_KEY => self.room_domain = None,
                SINCE_KEY => self.since = None,
                USER_DOMAIN_KEY => self.user_domain = None,
                USER_ID_KEY => self.user_id = None,
                USER_NAME_KEY => self.user_name = None,
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
        let value = match self
            .pddb
            .get(MTXCHAT_STATE, key, None, true, false, None, None::<fn()>)
        {
            Ok(mut pddb_key) => {
                let mut buffer = [0; 256];
                match pddb_key.read(&mut buffer) {
                    Ok(len) => match String::from_utf8(buffer[..len].to_vec()) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            log::warn!("failed to String: {:?}", e);
                            None
                        }
                    },
                    Err(e) => {
                        log::warn!("failed pddb_key read: {:?}", e);
                        None
                    }
                }
            }
            Err(_) => None,
        };
        log::info!("get '{}' = '{:?}'", key, value);
        Ok(value)
    }

    pub fn connect(&mut self) -> bool {
        log::info!("Attempting connect to Matrix server");
        if self.wifi() {
            if self.login() {
                if let Some(room) = self.get_room_id() {
                    self.dialogue_set(Some(room.as_str()));
                    self.listen();
                    if self.new_room {
                        self.new_room = false;
                        self.modals
                            .show_notification(t!("mtxchat.listen.patience", locales::LANG), None)
                            .expect("notification failed");
                    }
                    if self.new_username {
                        self.new_username = false;
                        self.help();
                    }
                    return true;
                } else {
                    self.modals
                        .show_notification(t!("mtxchat.roomid.failed", locales::LANG), None)
                        .expect("notification failed");
                }
            } else {
                self.modals
                    .show_notification(t!("mtxchat.login.failed", locales::LANG), None)
                    .expect("notification failed");
            }
        } else {
            self.modals
                .show_notification(t!("mtxchat.wifi.warning", locales::LANG), None)
                .expect("notification failed");
        }
        self.dialogue_set(None);
        false
    }

    pub fn login(&mut self) -> bool {
        self.token = self.get(TOKEN_KEY).unwrap_or(None);
        self.logged_in = false;
        let mut server = String::new();
        write!(
            server,
            "{}{}",
            HTTPS,
            &self
                .get(USER_DOMAIN_KEY)
                .unwrap_or(Some(DOMAIN_MATRIX.to_string()))
                .unwrap_or("".to_string())
        )
        .expect("failed to write server");
        if let Some(token) = &self.token {
            if let Some(user_id) = web::whoami(&server, &token, &mut self.agent) {
                let i = match user_id.find('@') {
                    Some(index) => index + 1,
                    None => 0,
                };
                let j = match user_id.find(':') {
                    Some(index) => index,
                    None => user_id.len(),
                };
                self.set(USER_ID_KEY, &user_id)
                    .expect("failed to save user id");
                self.set(USER_NAME_KEY, &user_id[i..j])
                    .expect("failed to save user name");
                self.set(USER_DOMAIN_KEY, &user_id[j + 1..])
                    .expect("failed to save user domain");
                self.logged_in = true;
            }
        }
        if !self.logged_in {
            if web::get_login_type(&server, &mut self.agent) {
                self.login_modal();
                let log_entry = match (&self.user_id, self.get(PASSWORD_KEY).unwrap_or(None)) {
                    (Some(user_id), Some(password)) => {
                        if let Some(new_token) =
                            web::authenticate_user(&server, &user_id, &password, &mut self.agent)
                        {
                            self.set_debug(TOKEN_KEY, &new_token);
                            self.logged_in = true;
                            "authenticated user"
                        } else {
                            "Error: cannnot login with password"
                        }
                    }
                    (None, _) => "missing user id",
                    (_, None) => "missing password",
                };
                log::info!("{log_entry}");
            } else {
                log::warn!("failed to web::get_login_type()");
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
        let mut builder = self
            .modals
            .alert_builder(t!("mtxchat.login.title", locales::LANG));
        let builder = match self.get(USER_NAME_KEY) {
            // TODO add TextValidationFn
            Ok(Some(user)) => builder.field_placeholder_persist(Some(user), None),
            _ => {
                self.new_username = true;
                builder.field(
                    Some(t!("mtxchat.user_name", locales::LANG).to_string()),
                    None,
                )
            }
        };
        let builder = match self.get(USER_DOMAIN_KEY) {
            // TODO add TextValidationFn
            Ok(Some(server)) => builder.field_placeholder_persist(Some(server), None),
            _ => builder.field(Some(t!("mtxchat.domain", locales::LANG).to_string()), None),
        };
        let builder = match self.get(PASSWORD_KEY) {
            Ok(Some(_pwd)) => builder.field_placeholder_persist(Some(HIDE.to_string()), None),
            _ => builder.field(
                Some(t!("mtxchat.password", locales::LANG).to_string()),
                None,
            ),
        };
        if let Ok(payloads) = builder.build() {
            self.unset_debug(TOKEN_KEY);
            if let Ok(content) = payloads.content()[0].content.as_str() {
                self.set(USER_NAME_KEY, content)
                    .expect("failed to save username");
            }
            if let Ok(content) = payloads.content()[1].content.as_str() {
                self.set(USER_DOMAIN_KEY, content)
                    .expect("failed to save server");
            }
            if let Ok(content) = payloads.content()[2].content.as_str() {
                if content.ne(HIDE) {
                    self.set(PASSWORD_KEY, content)
                        .expect("failed to save password");
                }
            }
            if let Some(user_name) = &self.user_name {
                if let Some(user_domain) = &self.user_domain {
                    let mut user_id = String::new();
                    write!(user_id, "@{}:{}", user_name, user_domain)
                        .expect("failed to write user_id");
                    self.set(USER_ID_KEY, &user_id)
                        .expect("failed to save user");
                }
            }
        }
        log::info!(
            "# user = {:?} user_name = {:?} server = {:?}",
            self.user_id,
            self.user_name,
            self.user_domain,
        );
    }

    pub fn logout(&mut self) {
        self.unset_debug(TOKEN_KEY);
        // TODO logout with server
    }

    pub fn get_room_id(&mut self) -> Option<String> {
        self.room_modal();
        let log_entry = match (
            self.logged_in,
            &self.token,
            &self.user_domain,
            &self.room_name,
            &self.room_domain,
        ) {
            (true, Some(token), Some(user_domain), Some(room_name), Some(room_domain)) => {
                let mut room = String::new();
                write!(room, "#{}:{}", &room_name, &room_domain).expect("failed to write room");
                let mut server = String::new();
                write!(server, "{}{}", HTTPS, &user_domain).expect("failed to write server");
                if let Some(room_id) = web::get_room_id(&server, &room, &token, &mut self.agent) {
                    self.set_debug(ROOM_ID_KEY, &room_id);
                    return Some(room);
                } else {
                    "failed to get room_id"
                }
            }
            (false, _, _, _, _) => "Not logged in",
            (_, None, _, _, _) => "No token set",
            (_, _, None, _, _) => "No user domain set",
            (_, _, _, None, _) => "No room name set",
            (_, _, _, _, None) => "No room domain set",
        };
        log::warn!("{log_entry}");
        None
    }

    pub fn redraw(&self) {
        self.chat.redraw();
    }

    pub fn room_modal(&mut self) {
        let mut builder = self
            .modals
            .alert_builder(t!("mtxchat.room.title", locales::LANG));
        let builder = match self.get(ROOM_NAME_KEY) {
            // TODO add TextValidationFn
            Ok(Some(room)) => builder.field_placeholder_persist(Some(room), None),
            _ => {
                self.new_room = true;
                builder.field(
                    Some(t!("mtxchat.room.name", locales::LANG).to_string()),
                    None,
                )
            }
        };
        let builder = match self.get(ROOM_DOMAIN_KEY) {
            // TODO add TextValidationFn
            Ok(Some(server)) => builder.field_placeholder_persist(Some(server), None),
            _ => builder.field(Some(t!("mtxchat.domain", locales::LANG).to_string()), None),
        };
        if let Ok(payloads) = builder.build() {
            self.unset_debug(ROOM_ID_KEY);
            self.unset_debug(SINCE_KEY);
            self.unset_debug(FILTER_KEY);
            if let Ok(content) = payloads.content()[0].content.as_str() {
                self.set(ROOM_NAME_KEY, content)
                    .expect("failed to save server");
            }
            if let Ok(content) = payloads.content()[1].content.as_str() {
                self.set(ROOM_DOMAIN_KEY, content)
                    .expect("failed to save server");
            }
        }
        log::info!(
            "# ROOM_NAME_KEY set '{}' => clearing ROOM_ID_KEY, SINCE_KEY, FILTER_KEY",
            ROOM_NAME_KEY
        );
    }

    pub fn dialogue_set(&self, room: Option<&str>) {
        self.chat
            .dialogue_set(MTXCHAT_DIALOGUE, room)
            .expect("failed to set dialogue");
    }

    pub fn help(&self) {
        self.chat.help();
    }

    pub fn get_filter(&mut self) -> bool {
        let log_entry = match (
            &self.filter,
            &self.logged_in,
            &self.token,
            &self.user_id,
            &self.user_domain,
            &self.room_id,
        ) {
            (Some(_filter), _, _, _, _, _) => "filter already set",
            (_, true, Some(token), Some(user_id), Some(user_domain), Some(room_id)) => {
                let mut server = String::new();
                write!(server, "{}{}", HTTPS, &user_domain).expect("failed to write server");
                log::info!(
                    "get_filter {} : {} : {} : {}",
                    &user_id,
                    &server,
                    &room_id,
                    &token
                );
                if let Some(new_filter) =
                    web::get_filter(&user_id, &server, &room_id, &token, &mut self.agent)
                {
                    if self.set_debug(FILTER_KEY, &new_filter) {
                        "set filter"
                    } else {
                        "failed to set"
                    }
                } else {
                    "failed to get filter"
                }
            }
            (_, false, _, _, _, _) => "Not logged in",
            (_, _, None, _, _, _) => "No token set",
            (_, _, _, None, _, _) => "No user id set",
            (_, _, _, _, None, _) => "No user domain set",
            (_, _, _, _, _, None) => "No room id set",
        };
        log::warn!("{log_entry}");
        self.filter.is_some()
    }

    pub fn listen(&mut self) {
        self.get_filter();
        let log_entry = match (
            self.listening,
            self.logged_in,
            &self.token,
            &self.room_id,
            &self.filter,
        ) {
            (false, true, Some(token), Some(room_id), Some(filter)) => {
                self.listening = true;
                std::thread::spawn({
                    let domain = self
                        .get(ROOM_DOMAIN_KEY)
                        .unwrap_or(Some(DOMAIN_MATRIX.to_string()))
                        .unwrap_or("".to_string());
                    let mut server = String::new();
                    write!(server, "{}{}", HTTPS, &domain,).expect("failed to write server");
                    let token = token.clone();
                    let room_id = room_id.clone();
                    let since = self.since.clone();
                    let filter = filter.clone();
                    let chat_cid = self.chat.cid().clone();
                    move || {
                        listen(
                            &server,
                            &token,
                            &room_id,
                            since.as_deref(),
                            &filter,
                            chat_cid,
                        );
                    }
                });
                "Started listening"
            }
            (true, _, _, _, _) => "Already listening",
            (_, false, _, _, _) => "Not logged in",
            (_, _, None, _, _) => "No token set",
            (_, _, _, None, _) => "No room id set",
            (_, _, _, _, None) => "No filter set",
        };
        log::info!("{log_entry}");
    }

    pub fn listen_over(&mut self, since: &str) {
        self.listening = false;
        log::info!("Stopped listening");
        if since.len() > 0 {
            self.set_debug(SINCE_KEY, since);
            // don't re-start listening if there was an error
            if self.logged_in && self.wifi() {
                self.listen();
            }
        }
    }

    pub fn gen_txn_id(&mut self) -> String {
        let mut txn_id = self.trng.get_u32().expect("unable to generate random u32");
        log::info!("trng.get_u32() = {}", txn_id);
        txn_id += SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .subsec_nanos();
        txn_id.to_string()
    }

    pub fn post(&mut self, text: &str) {
        let txn_id = self.gen_txn_id();
        let log_entry = match (
            self.logged_in,
            &self.token,
            &self.user_domain,
            &self.room_id,
        ) {
            (true, Some(token), Some(user_domain), Some(room_id)) => {
                log::info!("txn_id = {}", txn_id);
                let mut server = String::new();
                write!(server, "{}{}", HTTPS, &user_domain).expect("failed to write server");
                if web::send_message(&server, &room_id, &text, &txn_id, token, &mut self.agent) {
                    "SENT"
                } else {
                    "FAILED TO SEND"
                }
            }
            (false, _, _, _) => "Not logged in",
            (_, None, _, _) => "No token set",
            (_, _, None, _) => "No user domain set",
            (_, _, _, None) => "No room id set",
        };
        log::info!("{log_entry}");
    }

    // returns true is wifi is connected
    //
    // If wifi is not connected then a modal offers to "Connect to wifi?"
    // and tries for 10 seconds before representing.
    //
    pub fn wifi(&self) -> bool {
        if HOSTED_MODE {
            return true;
        }

        if let Some(conf) = self.netmgr.get_ipv4_config() {
            if conf.dhcp == com_rs::DhcpState::Bound {
                return true;
            }
        }

        while self.wifi_try_modal() {
            self.netmgr.connection_manager_wifi_on_and_run().unwrap();
            self.modals
                .start_progress("Connecting ...", 0, 10, 0)
                .expect("no progress bar");
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            for wait in 0..WIFI_TIMEOUT {
                tt.sleep_ms(1000).unwrap();
                self.modals
                    .update_progress(wait)
                    .expect("no progress update");
                if let Some(conf) = self.netmgr.get_ipv4_config() {
                    if conf.dhcp == com_rs::DhcpState::Bound {
                        self.modals
                            .finish_progress()
                            .expect("failed progress finish");
                        return true;
                    }
                }
            }
        }
        false
    }

    // returns true if "Connect to WiFi?" yes option is chosen
    //
    fn wifi_try_modal(&self) -> bool {
        self.modals.add_list_item("yes").expect("failed radio yes");
        self.modals.add_list_item("no").expect("failed radio no");
        self.modals
            .get_radiobutton("Connect to WiFi?")
            .expect("failed radiobutton modal");
        match self.modals.get_radio_index() {
            Ok(button) => button == 0,
            _ => false,
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
