use core::fmt::Write;
use core::str::FromStr;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write as StdWrite};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH}; // to help gen_txn_id

use xous::{Message, MessageEnvelope, StringBuffer};

mod migrations;
use migrations::run_migrations;
mod url;
mod web;

const DATE_2023: u64 = 1672538401; // for clock_not_set()

/// PDDB Dict for mtxcli keys
const MTXCLI_DICT: &str = "mtxcli";

const FILTER_KEY: &str = "_filter";
const PASSWORD_KEY: &str = "password";
const ROOM_ID_KEY: &str = "_room_id";
const ROOM_KEY: &str = "room";
const SINCE_KEY: &str = "_since";
const SERVER_KEY: &str = "server";
const TOKEN_KEY: &str = "_token";
const USER_KEY: &str = "user";
const USERNAME_KEY: &str = "username";
const VERSION_KEY: &str = "_version";
const CURRENT_VERSION_KEY: &str = "__version";

const HTTPS: &str = "https://";
const SERVER_MATRIX: &str = "https://matrix.org";

const EMPTY: &str = "";
const SENTINEL: char = '‡';
const MTX_LONG_TIMEOUT: i32 = 60000; // ms

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

/// Returns a version string which is more likely to string compare
/// correctly vs. another version. FFI please see
/// https://git-scm.com/docs/git-describe#_examples
fn get_version(ticktimer: &ticktimer_server::Ticktimer) -> String {
    let xous_version = ticktimer.get_version();
    let v: Vec<&str> = xous_version.split('-').collect();
    if v.len() > 2 {
        let n = v[1].parse::<usize>().expect("could not parse version");
        let version = format!("{}-{:04}", v[0], n);
        log::info!("version={}=", version);
        version
    } else {
        log::info!("ERROR, couldn't find version from xous_version: {}", xous_version);
        xous_version
    }
}

/////////////////////////// Common items to all commands
pub trait ShellCmdApi<'a> {
    // user implemented:
    // called to process the command with the remainder of the string attached
    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error>;
    // called to process incoming messages that may have been origniated by the most recently issued command
    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        log::info!("received unhandled message {:?}", msg);
        Ok(None)
    }

    // created with cmd_api! macro
    // checks if the command matches the current verb in question
    fn matches(&self, verb: &str) -> bool;
    // returns my verb
    fn verb(&self) -> &'static str;
}
// the argument to this macro is the command verb
macro_rules! cmd_api {
    ($verb:expr) => {
        fn verb(&self) -> &'static str { stringify!($verb) }
        fn matches(&self, verb: &str) -> bool { if verb == stringify!($verb) { true } else { false } }
    };
}

use trng::*;
/////////////////////////// Command shell integration
#[derive(Debug)]
pub struct CommonEnv {
    ticktimer: ticktimer_server::Ticktimer,
    cb_registrations: HashMap<u32, String>,
    async_msg_callback_id: u32,
    async_msg_conn: u32,
    trng: Trng,
    netmgr: net::NetManager,
    xns: xous_names::XousNames,
    user: String,
    username: String,
    server: String,
    token: String,
    logged_in: bool,
    room_id: String,
    filter: String,
    since: String,
    first_line: bool,
    version: String,
    initialized: bool,
    wifi_connected: bool,
    listening: bool,
}
impl CommonEnv {
    pub fn new() -> CommonEnv {
        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
        let cb_registrations = HashMap::new();
        let async_msg_callback_id: u32 = 0;
        let async_msg_conn: u32 = 0;
        let netmgr = net::NetManager::new();
        let xns = xous_names::XousNames::new().unwrap();
        let trng = Trng::new(&xns).unwrap();
        let common = CommonEnv {
            ticktimer,
            cb_registrations,
            async_msg_callback_id,
            async_msg_conn,
            trng,
            netmgr,
            xns,
            user: EMPTY.to_string(),
            username: EMPTY.to_string(),
            server: SERVER_MATRIX.to_string(),
            token: EMPTY.to_string(),
            logged_in: false,
            room_id: EMPTY.to_string(),
            filter: EMPTY.to_string(),
            since: EMPTY.to_string(),
            first_line: true,
            version: EMPTY.to_string(),
            initialized: false,
            wifi_connected: false,
            listening: false,
        };
        common
    }

    pub fn register_handler(&mut self, verb: String) -> u32 {
        let mut key: u32;
        loop {
            key = self.trng.get_u32().unwrap();
            // reserve the bottom 1000 IDs for the main loop enums.
            if !self.cb_registrations.contains_key(&key) && (key > 1000) {
                break;
            }
        }
        self.cb_registrations.insert(key, verb);
        key
    }

    // NOTE: Here the async callbacks will be managed by the command "help"
    // as it is already an unsual command
    pub fn register_async_msg(&mut self) {
        self.async_msg_conn = self.xns.request_connection_blocking(crate::SERVER_NAME_MTXCLI).unwrap();
        self.async_msg_callback_id = self.register_handler(String::from("help"));
    }

    pub fn scalar_async_msg(&self, async_msg_id: usize) {
        let msg = Message::new_scalar(self.async_msg_callback_id as usize, 0, 0, 0, async_msg_id);
        xous::send_message(self.async_msg_conn, msg).unwrap();
    }

    pub fn send_async_msg(&self, async_msg: &str) {
        let str_buf = StringBuffer::from_str(async_msg).expect("unable to create string message");
        str_buf.send(self.async_msg_conn, self.async_msg_callback_id).expect("unable to send string message");
    }

    pub fn gen_txn_id(&mut self) -> String {
        let mut txn_id = self.trng.get_u32().expect("unable to generate random u32");
        log::info!("trng.get_u32() = {}", txn_id);
        txn_id += SystemTime::now().duration_since(UNIX_EPOCH).expect("Time went backwards").subsec_nanos();
        txn_id.to_string()
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(ErrorKind::PermissionDenied, "may not set a variable beginning with __ "))
        } else {
            log::info!("set '{}' = '{}'", key, value);
            let mut keypath = PathBuf::new();
            keypath.push(MTXCLI_DICT);
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                // log::info!("dict '{}' exists", MTXCLI_DICT);
            } else {
                log::info!("dict '{}' does NOT exist.. creating it", MTXCLI_DICT);
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
        log::info!("# user = '{}' username = '{}' server = '{}'", self.user, self.username, self.server);
        self.set_debug(USERNAME_KEY, &self.username.clone());
        self.set_debug(SERVER_KEY, &self.server.clone());
        self.unset_debug(TOKEN_KEY);
    }

    pub fn set_password(&mut self) {
        log::info!("# PASSWORD_KEY set '{}' => clearing TOKEN_KEY", PASSWORD_KEY);
        self.unset_debug(TOKEN_KEY);
    }

    pub fn set_room(&mut self) {
        log::info!("# ROOM_KEY set '{}' => clearing ROOM_ID_KEY, SINCE_KEY, FILTER_KEY", ROOM_KEY);
        self.unset_debug(ROOM_ID_KEY);
        self.unset_debug(SINCE_KEY);
        self.unset_debug(FILTER_KEY);
    }

    pub fn unset(&mut self, key: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(ErrorKind::PermissionDenied, "may not unset a variable beginning with __ "))
        } else {
            log::info!("unset '{}'", key);
            let mut keypath = PathBuf::new();
            keypath.push(MTXCLI_DICT);
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                // log::info!("dict '{}' exists", MTXCLI_DICT);
            } else {
                log::info!("dict '{}' does NOT exist.. creating it", MTXCLI_DICT);
                std::fs::create_dir_all(&keypath)?;
            }
            keypath.push(key);
            if std::fs::metadata(&keypath).is_ok() {
                // keypath exists
                log::info!("dict:key = '{}:{}' exists.. deleting it", MTXCLI_DICT, key);
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

    pub fn get(&mut self, key: &str) -> Result<Option<String>, Error> {
        if key.eq(CURRENT_VERSION_KEY) {
            Ok(Some(self.version.clone()))
        } else {
            let mut keypath = PathBuf::new();
            keypath.push(MTXCLI_DICT);
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                // log::info!("dict '{}' exists", MTXCLI_DICT);
            } else {
                log::info!("dict '{}' does NOT exist.. creating it", MTXCLI_DICT);
                std::fs::create_dir_all(&keypath)?;
            }
            keypath.push(key);
            if let Ok(mut file) = File::open(keypath) {
                let mut value = String::new();
                file.read_to_string(&mut value)?;
                log::info!("get '{}' = '{}'", key, value);
                Ok(Some(value))
            } else {
                Ok(None)
            }
        }
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

    pub fn user_says(&mut self, text: &str) {
        if !self.logged_in {
            if !self.login() {
                return;
            }
        }
        if self.room_id.len() == 0 {
            if !self.get_room_id() {
                return;
            }
        }
        if self.filter.len() == 0 {
            if !self.get_filter() {
                return;
            }
        }
        let txn_id = self.gen_txn_id();
        log::info!("txn_id = {}", txn_id);
        if web::send_message(&self.server, &self.room_id, &text, &txn_id, &self.token) {
            log::info!("SENT: {}", text);
        } else {
            log::info!("FAILED TO SEND");
            self.scalar_async_msg(FAILED_TO_SEND_ID);
        }
    }

    pub fn login(&mut self) -> bool {
        self.scalar_async_msg(LOGGING_IN_ID);
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
                    self.scalar_async_msg(SET_USER_ID);
                }
                let password = self.get_default(PASSWORD_KEY, EMPTY);
                if password.len() == 0 {
                    self.scalar_async_msg(SET_PASSWORD_ID);
                } else {
                    if let Some(new_token) = web::authenticate_user(&self.server, &user, &password) {
                        self.set_debug(TOKEN_KEY, &new_token);
                        self.logged_in = true;
                    } else {
                        log::info!("Error: cannnot login with type: {}", web::MTX_LOGIN_PASSWORD);
                    }
                }
            }
        }
        if self.logged_in {
            self.scalar_async_msg(LOGGED_IN_ID);
        } else {
            self.scalar_async_msg(LOGIN_FAILED_ID);
        }
        self.logged_in
    }

    pub fn logout(&mut self) {
        self.unset_debug(TOKEN_KEY);
        self.scalar_async_msg(LOGGED_OUT_ID);
        self.logged_in = false;
    }

    // assume logged in, token is valid
    pub fn get_room_id(&mut self) -> bool {
        if self.room_id.len() > 0 {
            true
        } else {
            let room = self.get_default(ROOM_KEY, EMPTY);
            let server = self.get_default(SERVER_KEY, EMPTY);
            if room.len() == 0 {
                self.scalar_async_msg(SET_ROOM_ID);
                false
            } else if server.len() == 0 {
                self.scalar_async_msg(SET_SERVER_ID);
                false
            } else {
                let mut room_server = String::new();
                if !room.starts_with("#") {
                    room_server.push_str("#");
                }
                room_server.push_str(&room);
                room_server.push_str(":");
                let i = match server.find(HTTPS) {
                    Some(index) => index + HTTPS.len(),
                    None => server.len(),
                };
                if i >= server.len() {
                    self.scalar_async_msg(SET_SERVER_ID);
                    false
                } else {
                    room_server.push_str(&server[i..]);
                    if let Some(new_room_id) = web::get_room_id(&self.server, &room_server, &self.token) {
                        self.set_debug(ROOM_ID_KEY, &new_room_id);
                        true
                    } else {
                        self.scalar_async_msg(ROOMID_FAILED_ID);
                        false
                    }
                }
            }
        }
    }

    // assume logged in, token is valid, room_id is valid, user is valid
    pub fn get_filter(&mut self) -> bool {
        if self.filter.len() > 0 {
            true
        } else {
            if let Some(new_filter) = web::get_filter(&self.user, &self.server, &self.room_id, &self.token) {
                self.set_debug(FILTER_KEY, &new_filter);
                true
            } else {
                self.scalar_async_msg(FILTER_FAILED_ID);
                false
            }
        }
    }

    pub fn listen(&mut self) {
        if self.listening {
            log::info!("Already listening");
            return;
        }
        if !self.logged_in {
            log::info!("Not logged in");
            return;
        }
        if self.room_id.len() == 0 {
            if !self.get_room_id() {
                return;
            }
        }
        if self.filter.len() == 0 {
            if !self.get_filter() {
                return;
            }
        }
        self.listening = true;
        log::info!("Started listening");
        std::thread::spawn({
            let server = self.server.clone();
            let filter = self.filter.clone();
            let since = self.since.clone();
            let room_id = self.room_id.clone();
            let token = self.token.clone();
            let async_msg_conn = self.async_msg_conn.clone();
            let async_msg_callback_id = self.async_msg_callback_id.clone();
            move || {
                // log::info!("client_sync for {} ms...", MTX_LONG_TIMEOUT);
                let mut response = String::new();
                response.push(SENTINEL);
                if let Some((since, messages)) =
                    web::client_sync(&server, &filter, &since, MTX_LONG_TIMEOUT, &room_id, &token)
                {
                    response.push_str(&since);
                    response.push(SENTINEL);
                    response.push_str(&messages);
                    response.push(SENTINEL);
                }
                let str_buf = StringBuffer::from_str(&response).expect("unable to create string message");
                str_buf.send(async_msg_conn, async_msg_callback_id).expect("unable to send string message");
            }
        });
    }

    pub fn listen_over(&mut self, since: &str) {
        self.listening = false;
        log::info!("Stopped listening");
        if since.len() > 0 {
            self.set_debug(SINCE_KEY, since);
            // don't re-start listening if there was an error
            if self.logged_in && (HOSTED_MODE || self.wifi_connected) {
                self.listen();
            }
        }
    }
}

/*
    To add a new command:
        0. ensure that the command implements the ShellCmdApi (above)
        1. mod/use the new command
        2. create an entry for the command's storage in the CmdEnv structure
        3. initialize the persistant storage here
        4. add it to the "commands" array in the dispatch() routine below

    Side note: if your command doesn't require persistent storage, you could,
    technically, generate the command dynamically every time it's called.
*/

///// 1. add your module here, and pull its namespace into the local crate
mod get;
use get::*;
mod heap;
use heap::*;
mod help;
use help::*;
mod login;
use login::*;
mod logout;
use logout::*;
mod set;
use set::*;
mod status;
use status::*;
mod unset;
use unset::*;

pub struct CmdEnv {
    common_env: CommonEnv,
    lastverb: String,
    ///// 2. declare storage for your command here.
    get_cmd: Get,
    heap_cmd: Heap,
    help_cmd: Help,
    login_cmd: Login,
    logout_cmd: Logout,
    set_cmd: Set,
    status_cmd: Status,
    unset_cmd: Unset,
}
impl CmdEnv {
    pub fn new() -> CmdEnv {
        let mut common_env = CommonEnv::new();
        common_env.register_async_msg();
        CmdEnv {
            common_env,
            lastverb: String::new(),
            ///// 3. initialize your storage, by calling new()
            get_cmd: Get::new(),
            heap_cmd: Heap::new(),
            help_cmd: Help::new(),
            login_cmd: Login::new(),
            logout_cmd: Logout::new(),
            set_cmd: Set::new(),
            status_cmd: Status::new(),
            unset_cmd: Unset::new(),
        }
    }

    pub fn dispatch(
        &mut self,
        maybe_cmdline: Option<&mut String>,
        maybe_callback: Option<&MessageEnvelope>,
    ) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        self.common_env.first_line = true;

        let commands: &mut [&mut dyn ShellCmdApi] = &mut [
            ///// 4. add your command to this array, so that it can be looked up and dispatched
            &mut self.get_cmd,
            &mut self.heap_cmd,
            &mut self.help_cmd,
            &mut self.login_cmd,
            &mut self.logout_cmd,
            &mut self.set_cmd,
            &mut self.status_cmd,
            &mut self.unset_cmd,
        ];

        let prev_wifi_connected = self.common_env.wifi_connected;
        self.common_env.wifi_connected = false;
        if let Some(conf) = self.common_env.netmgr.get_ipv4_config() {
            if conf.dhcp == com_rs::DhcpState::Bound {
                self.common_env.wifi_connected = true;
            }
        }
        if self.common_env.wifi_connected != prev_wifi_connected {
            if self.common_env.wifi_connected {
                self.common_env.scalar_async_msg(WIFI_CONNECTED_ID);
            } else {
                self.common_env.scalar_async_msg(WIFI_NOT_CONNECTED_ID);
            }
        }

        if let Some(cmdline) = maybe_cmdline {
            if !self.common_env.initialized {
                log::info!("initializing");
                log::info!("WiFi connected: {}", self.common_env.wifi_connected);
                if clock_not_set() {
                    self.common_env.scalar_async_msg(CLOCK_NOT_SET_ID);
                }
                if !self.common_env.wifi_connected && !prev_wifi_connected {
                    self.common_env.scalar_async_msg(WIFI_NOT_CONNECTED_ID);
                }
                let pddb = pddb::PddbMountPoller::new();
                if !pddb.is_mounted_nonblocking() {
                    self.common_env.scalar_async_msg(PDDB_NOT_MOUNTED_ID);
                } else {
                    log::info!("PDDB is mounted");
                    self.common_env.user = self.common_env.get_default(USER_KEY, EMPTY);
                    self.common_env.username = self.common_env.get_default(USERNAME_KEY, USERNAME_KEY);
                    self.common_env.server = self.common_env.get_default(SERVER_KEY, SERVER_MATRIX);
                    self.common_env.room_id = self.common_env.get_default(ROOM_ID_KEY, EMPTY);
                    self.common_env.filter = self.common_env.get_default(FILTER_KEY, EMPTY);
                    self.common_env.since = self.common_env.get_default(SINCE_KEY, EMPTY);
                    self.common_env.version = get_version(&self.common_env.ticktimer);
                    if self.common_env.logged_in || self.common_env.login() {
                        run_migrations(&mut self.common_env);
                        self.common_env.initialized = true;
                        log::info!("initialized");
                        self.common_env.scalar_async_msg(MTXCLI_INITIALIZED_ID);
                    } else {
                        self.common_env.scalar_async_msg(PLEASE_LOGIN_ID);
                    }
                }
            }

            if !self.common_env.listening
                && self.common_env.logged_in
                && (HOSTED_MODE || self.common_env.wifi_connected)
                && self.common_env.initialized
            {
                self.common_env.listen();
            }

            let maybe_verb = tokenize(cmdline);

            let mut cmd_ret: Result<Option<String>, xous::Error> = Ok(None);
            if let Some(verb_string) = maybe_verb {
                let verb = &verb_string;

                // if verb starts with a slash then it's a command (else chat)
                if verb.starts_with("/") {
                    // search through the list of commands linearly until one
                    // matches, then run it.
                    let command = &verb[1..];
                    let mut match_found = false;
                    for cmd in commands.iter_mut() {
                        if cmd.matches(command) {
                            match_found = true;
                            cmd_ret = cmd.process(cmdline.to_string(), &mut self.common_env);
                            self.lastverb.clear();
                            write!(self.lastverb, "{}", verb).expect("couldn't record last verb");
                        };
                    }

                    // if none match, create a list of available commands
                    if !match_found {
                        let mut first = true;
                        write!(ret, "Commands: ").unwrap();
                        for cmd in commands.iter() {
                            if !first {
                                ret.push_str(", ");
                            }
                            ret.push_str("/");
                            ret.push_str(cmd.verb());
                            first = false;
                        }
                        Ok(Some(ret))
                    } else {
                        cmd_ret
                    }
                } else {
                    // chat
                    let mut text = String::from(verb);
                    text.push_str(" ");
                    text.push_str(cmdline);
                    self.common_env.user_says(&text);
                    // only for sync case Ok(Some(ret))
                    Ok(None)
                }
            } else {
                log::info!("NO INPUT");
                Ok(None)
            }
        } else if let Some(callback) = maybe_callback {
            let mut cmd_ret: Result<Option<String>, xous::Error> = Ok(None);
            // first check and see if we have a callback registration; if not, just map to the last verb
            let verb = match self.common_env.cb_registrations.get(&(callback.body.id() as u32)) {
                Some(verb) => &verb,
                None => &self.lastverb,
            };
            // now dispatch
            let mut verbfound = false;
            for cmd in commands.iter_mut() {
                if cmd.matches(verb) {
                    cmd_ret = cmd.callback(callback, &mut self.common_env);
                    verbfound = true;
                    break;
                };
            }
            if verbfound { cmd_ret } else { Ok(None) }
        } else {
            Ok(None)
        }
    }
}

/// extract the first token, as delimited by spaces
/// modifies the incoming line by removing the token and returning the remainder
/// returns the found token
pub fn tokenize(line: &mut String) -> Option<String> {
    let mut token = String::new();
    let mut retline = String::new();

    let lineiter = line.chars();
    let mut foundspace = false;
    let mut foundrest = false;
    for ch in lineiter {
        if ch != ' ' && !foundspace {
            token.push(ch);
        } else if foundspace && foundrest {
            retline.push(ch);
        } else if foundspace && ch != ' ' {
            // handle case of multiple spaces in a row
            foundrest = true;
            retline.push(ch);
        } else {
            foundspace = true;
            // consume the space
        }
    }
    line.clear();
    write!(line, "{}", retline).unwrap();
    if token.len() > 0 { Some(token) } else { None }
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

pub fn clock_not_set() -> bool {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    let seconds: u64 = since_the_epoch.as_secs();
    if seconds < DATE_2023 {
        log::info!("clock NOT set, seconds since epoch: {}", seconds);
        true
    } else {
        log::info!("clock set, seconds since epoch: {}", seconds);
        false
    }
}
