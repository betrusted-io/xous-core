use xous::{MessageEnvelope, Message,StringBuffer};
use xous_ipc::String as XousString;
use core::fmt::Write;
use locales::t;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write as StdWrite, Error, ErrorKind};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH}; // to help gen_txn_id
use core::str::FromStr;

mod migrations;  use migrations::run_migrations;
mod url;
mod web;

const DATE_2023: u64 = 1672538401; // for clock_not_set()
const APP: &str = "mtxcli";

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
const MTX_TIMEOUT: i32 = 300; // ms

pub const CLOCK_NOT_SET_ID: usize = 1;

/// Returns a version string which is more likely to string compare
/// correctly vs. another version. FFI please see
/// https://git-scm.com/docs/git-describe#_examples
fn get_version(ticktimer: &ticktimer_server::Ticktimer) -> String {
    let xous_version = ticktimer.get_version();
    let v: Vec<&str> = xous_version.split('-').collect();
    if v.len() > 2 {
        let n = v[1].parse::<usize>().expect("could not parse version");
        let version = format!("{}-{:04}",v[0], n);
        log::info!("version={}=", version);
        version
    } else {
        log::error!("ERROR, couldn't find version from xous_version: {}",
                    xous_version);
        xous_version
    }
}

/////////////////////////// Common items to all commands
pub trait ShellCmdApi<'a> {
    // user implemented:
    // called to process the command with the remainder of the string attached
    fn process(&mut self, args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error>;
    // called to process incoming messages that may have been origniated by the most recently issued command
    fn callback(&mut self, msg: &MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
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
        fn verb(&self) -> &'static str {
            stringify!($verb)
        }
        fn matches(&self, verb: &str) -> bool {
            if verb == stringify!($verb) {
                true
            } else {
                false
            }
        }
    };
}

use trng::*;
/////////////////////////// Command shell integration
#[derive(Debug)]
pub struct CommonEnv {
    ticktimer: ticktimer_server::Ticktimer,
    cb_registrations: HashMap::<u32, XousString::<256>>,
    async_msg_callback_id: u32,
    async_msg_conn: u32,
    trng: Trng,
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
}
impl CommonEnv {
    pub fn new() -> CommonEnv {
        let xns = xous_names::XousNames::new().unwrap();
        let ticktimer = ticktimer_server::Ticktimer::new()
            .expect("Couldn't connect to Ticktimer");
        let cb_registrations = HashMap::new();
        let async_msg_callback_id: u32 = 0;
        let async_msg_conn: u32 = 0;
        let common = CommonEnv {
            ticktimer,
            cb_registrations,
            async_msg_callback_id,
            async_msg_conn,
            trng: Trng::new(&xns).unwrap(),
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
        };
        common
    }

    pub fn register_handler(&mut self, verb: XousString::<256>) -> u32 {
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
        self.async_msg_callback_id = self.register_handler(XousString::<256>::from_str("help"));
    }

    pub fn scalar_async_msg(&self, async_msg_id: usize) {
        let msg = Message::new_scalar(self.async_msg_callback_id as usize,
                                      0, 0, 0, async_msg_id);
        xous::send_message(self.async_msg_conn, msg).unwrap();
    }

    pub fn send_async_msg(&self, async_msg: &str) {
        let str_buf = StringBuffer::from_str(async_msg)
            .expect("unable to create string message");
        str_buf.send(self.async_msg_conn, self.async_msg_callback_id)
            .expect("unable to send string message");
        // let msg = str_buf.create_memory_message(self.async_msg_callback_id);
        // xous::send_message(self.async_msg_conn, msg).unwrap();
    }

    pub fn gen_txn_id(&mut self) -> String {
        let mut txn_id = self.trng.get_u32()
            .expect("unable to generate random u32");
        log::info!("trng.get_u32() = {}", txn_id);
        txn_id += SystemTime::now().duration_since(UNIX_EPOCH)
            .expect("Time went backwards").subsec_nanos();
        txn_id.to_string()
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(ErrorKind::PermissionDenied,
                           "may not set a variable beginning with __ "))
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
            match key { // special case side effects
                FILTER_KEY => { self.filter = value.to_string(); }
                PASSWORD_KEY => { self.set_password(); }
                ROOM_ID_KEY => { self.room_id = value.to_string(); }
                ROOM_KEY => { self.set_room(); }
                SERVER_KEY => { self.server = value.to_string(); }
                SINCE_KEY => { self.since = value.to_string(); }
                USERNAME_KEY => { self.username = value.to_string(); }
                USER_KEY => { self.set_user(value); }
                _ => { }
            }
            Ok(())
        }
    }

    // will log on error (vs. panic)
    pub fn set_debug(&mut self, key: &str, value: &str) -> bool {
        match self.set(key, value) {
            Ok(()) => {
                true
            },
            Err(e) => {
                log::error!("error setting key {}: {:?}", key, e);
                false
            }
        }
    }

    pub fn set_user(&mut self, value: &str) {
        log::debug!("# USER_KEY set '{}' = '{}'", USER_KEY, value);
        let i = match value.find('@') {
            Some(index) => { index + 1 },
            None => { 0 },
        };
        let j = match value.find(':') {
            Some(index) => { index },
            None => { value.len() },
        };
        self.username = (&value[i..j]).to_string();
        if j < value.len() {
            self.server = String::from(HTTPS);
            self.server.push_str(&value[j + 1..]);
        } else {
            self.server = SERVER_MATRIX.to_string();
        }
        self.user = value.to_string();
        log::debug!("# user = '{}' username = '{}' server = '{}'", self.user, self.username, self.server);
        self.set_debug(USERNAME_KEY, &self.username.clone());
        self.set_debug(SERVER_KEY, &self.server.clone());
        self.unset_debug(TOKEN_KEY);
        self.token = EMPTY.to_string();
    }

    pub fn set_password(&mut self) {
        log::debug!("# PASSWORD_KEY set '{}' => clearing TOKEN_KEY", PASSWORD_KEY);
        self.unset_debug(TOKEN_KEY);
    }

    pub fn set_room(&mut self) {
        log::debug!("# ROOM_KEY set '{}' => clearing ROOM_ID_KEY, SINCE_KEY, FILTER_KEY", ROOM_KEY);
        self.unset_debug(ROOM_ID_KEY);
        self.room_id = EMPTY.to_string();
        self.unset_debug(SINCE_KEY);
        self.since = EMPTY.to_string();
        self.unset_debug(FILTER_KEY);
        self.filter = EMPTY.to_string();
    }

    pub fn unset(&mut self, key: &str) -> Result<(), Error> {
        if key.starts_with("__") {
            Err(Error::new(ErrorKind::PermissionDenied,
                           "may not unset a variable beginning with __ "))
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
            if std::fs::metadata(&keypath).is_ok() { // keypath exists
                log::info!("dict:key = '{}:{}' exists.. deleting it", MTXCLI_DICT, key);

                std::fs::remove_file(keypath)?;
            }
            match key { // special case side effects -- update cached values
                FILTER_KEY => { self.filter = EMPTY.to_string(); }
                ROOM_ID_KEY => { self.room_id = EMPTY.to_string(); }
                SINCE_KEY => { self.since = EMPTY.to_string(); }
                SERVER_KEY => { self.server = EMPTY.to_string(); }
                USER_KEY => { self.user = EMPTY.to_string(); }
                USERNAME_KEY => { self.username = EMPTY.to_string(); }
                _ => { }
            }
            Ok(())
        }
    }

    // will log on error (vs. panic)
    pub fn unset_debug(&mut self, key: &str) -> bool {
        match self.unset(key) {
            Ok(()) => {
                true
            },
            Err(e) => {
                log::error!("error unsetting key {}: {:?}", key, e);
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
            if let Ok(mut file)= File::open(keypath) {
                let mut value = String::new();
                file.read_to_string(&mut value)?;
                // log::info!("get '{}' = '{}'", key, value);
                Ok(Some(value))
            } else {
                Ok(None)
            }
        }
    }

    pub fn get_default(&mut self, key: &str, default: &str) -> String {
        match self.get(key) {
            Ok(None) => {
                default.to_string()
            },
            Ok(Some(value)) => {
                value.to_string()
            }
            Err(e) => {
                log::error!("error getting key {}: {:?}", key, e);
                default.to_string()
            }
        }
    }

    pub fn prompt(&mut self, ret: &mut XousString::<1024>, text: &str) {
        if self.first_line {
            write!(ret, "{}> {}", APP, text).unwrap();
            self.first_line = false;
        } else {
            write!(ret, "\n{}> {}", APP, text).unwrap();
        }
    }

    pub fn user_says(&mut self, ret: &mut XousString::<1024>, text: &str) {
        if ! self.logged_in {
            if ! self.login(ret) {
                return;
            }
        }
        if self.room_id.len() == 0 {
            if ! self.get_room_id(ret) {
                return;
            }
        }
        if self.filter.len() == 0 {
            if ! self.get_filter(ret) {
                return;
            }
        }
        self.read_messages(ret);
        // The following is not required, because we will get what
        // the user said when we read_messages the second time
        // write!(ret, "{}> {}", self.username, text).unwrap();

        let txn_id = self.gen_txn_id();
        log::info!("txn_id = {}", txn_id);
        if web::send_message(&self.server, &self.room_id, &text, &txn_id, &self.token) {
            log::info!("SENT: {}", text);
            self.read_messages(ret); // update since to include what user said
        } else {
            log::info!("FAILED TO SEND");
            write!(ret, "{}", t!("mtxcli.send.failed", xous::LANG)).unwrap();
        }
    }

    pub fn login(&mut self, ret: &mut XousString::<1024>) -> bool {
        self.prompt(ret, t!("mtxcli.logging.in", xous::LANG));
        self.token = self.get_default(TOKEN_KEY, EMPTY);
        self.logged_in = false;
        if self.token.len() > 0 {
            if web::whoami(&self.server, &self.token) {
                self.logged_in = true;
            }
        }
        if ! self.logged_in {
            if web::get_login_type(&self.server) {
                let user = self.get_default(USER_KEY, USER_KEY);
                let password = self.get_default(PASSWORD_KEY, EMPTY);
                if password.len() == 0 {
                    self.prompt(ret, t!("mtxcli.please.set.user", xous::LANG));
                    self.prompt(ret, t!("mtxcli.please.set.password", xous::LANG));
                } else {
                    if let Some(new_token) = web::authenticate_user(&self.server, &user, &password) {
                        self.set_debug(TOKEN_KEY, &new_token);
                        self.token = new_token;
                        self.logged_in = true;
                    } else {
                       log::error!("Error: cannnot login with type: {}", web::MTX_LOGIN_PASSWORD);
                    }
                }
            }
        }
        if self.logged_in {
            self.prompt(ret, t!("mtxcli.logged.in", xous::LANG));
        } else {
            self.prompt(ret, t!("mtxcli.login.failed", xous::LANG));
        }
        self.logged_in
    }

    pub fn logout(&mut self, ret: &mut XousString::<1024>) {
        self.unset_debug(TOKEN_KEY);
        self.prompt(ret, t!("mtxcli.logged.out", xous::LANG));
        self.logged_in = false;
    }

    // assume logged in, token is valid
    pub fn get_room_id(&mut self, ret: &mut XousString::<1024>) -> bool {
        if self.room_id.len() > 0 {
            true
        } else {
            let room = self.get_default(ROOM_KEY, EMPTY);
            let server = self.get_default(SERVER_KEY, EMPTY);
            if room.len() == 0 {
                self.prompt(ret, t!("mtxcli.please.set.room", xous::LANG));
                false
            } else if server.len() == 0 {
                self.prompt(ret, t!("mtxcli.please.set.server", xous::LANG));
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
                    self.prompt(ret, t!("mtxcli.please.set.server", xous::LANG));
                    false
                } else {
                    room_server.push_str(&server[i..]);
                    if let Some(new_room_id) = web::get_room_id(&self.server, &room_server, &self.token) {
                        self.set_debug(ROOM_ID_KEY, &new_room_id);
                        self.room_id = new_room_id;
                        true
                    } else {
                        self.prompt(ret, t!("mtxcli.roomid.failed", xous::LANG));
                        false
                    }
                }
            }
        }
    }

    // assume logged in, token is valid, room_id is valid, user is valid
    pub fn get_filter(&mut self, ret: &mut XousString::<1024>) -> bool {
        if self.filter.len() > 0 {
            true
        } else {
            if let Some(new_filter) = web::get_filter(&self.user, &self.server,
                                                      &self.room_id, &self.token) {
                self.set_debug(FILTER_KEY, &new_filter);
                self.filter = new_filter;
                true
            } else {
                self.prompt(ret, t!("mtxcli.filter.failed", xous::LANG));
                false
            }
        }
    }

    // assume logged in, token is valid, room_id is valid, user is valid,
    // and filter is valid
    pub fn read_messages(&mut self, ret: &mut XousString::<1024>) -> bool {
        if let Some((since, messages)) = web::client_sync(&self.server, &self.filter,
                                                          &self.since, MTX_TIMEOUT,
                                                          &self.room_id, &self.token) {
            self.set_debug(SINCE_KEY, &since);
            self.since = since;
            if messages.len() > 0 {
                if self.first_line {
                    write!(ret, "{}", messages).unwrap();
                    self.first_line = false;
                } else {
                    write!(ret, "\n{}", messages).unwrap();
                }
                true
            } else {
                false
            }
        } else {
            false
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
mod get;       use get::*;
mod heap;      use heap::*;
mod help;      use help::*;
mod login;     use login::*;
mod logout;    use logout::*;
mod set;       use set::*;
mod status;    use status::*;
mod unset;     use unset::*;

pub struct CmdEnv {
    common_env: CommonEnv,
    lastverb: XousString::<256>,
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
            lastverb: XousString::<256>::new(),
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

    pub fn dispatch(&mut self, maybe_cmdline: Option<&mut XousString::<1024>>, maybe_callback: Option<&MessageEnvelope>) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        self.common_env.first_line = true;

        let commands: &mut [& mut dyn ShellCmdApi] = &mut [
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

        if let Some(cmdline) = maybe_cmdline {
            // Initialization (must wait until PDDB is mounted and llio can
            // report accurate time). Here we wait to check for initialization
            // once the user has typed something
            if ! self.initialized {
                log::info!("initializing");
                if clock_not_set() {
                    self.common_env.scalar_async_msg(CLOCK_NOT_SET_ID);
                }
                self.common_env.user = self.common_env.get_default(USER_KEY, EMPTY);
                self.common_env.username = self.common_env.get_default(USERNAME_KEY, USERNAME_KEY);
                self.common_env.server = self.common_env.get_default(SERVER_KEY, SERVER_MATRIX);
                self.common_env.room_id = self.common_env.get_default(ROOM_ID_KEY, EMPTY);
                self.common_env.filter = self.common_env.get_default(FILTER_KEY, EMPTY);
                self.common_env.since = self.common_env.get_default(SINCE_KEY, EMPTY);
                self.common_env.version = get_version(&self.common_env.ticktimer);

                run_migrations(&mut self.common_env);
                self.initialized = true;
            }

            let maybe_verb = tokenize(cmdline);

            let mut cmd_ret: Result<Option<XousString::<1024>>, xous::Error> = Ok(None);
            if let Some(verb_string) = maybe_verb {
                let verb = verb_string.to_str();

                // if verb starts with a slash then it's a command (else chat)
                if verb.starts_with("/") {
                    // search through the list of commands linearly until one
                    // matches, then run it.
                    let command = &verb[1..];
                    let mut match_found = false;
                    for cmd in commands.iter_mut() {
                        if cmd.matches(command) {
                            match_found = true;
                            cmd_ret = cmd.process(*cmdline, &mut self.common_env);
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
                                ret.append(", ")?;
                            }
                            ret.append("/")?;
                            ret.append(cmd.verb())?;
                            first = false;
                        }
                        Ok(Some(ret))
                    } else {
                        cmd_ret
                    }
                } else { // chat
                    let mut text = String::from(verb);
                    text.push_str(" ");
                    text.push_str(cmdline.to_str());
                    self.common_env.user_says(&mut ret, &text);
                    Ok(Some(ret))
                }
            } else {
                log::info!("NO INPUT");
                if self.common_env.read_messages(&mut ret) {
                    Ok(Some(ret))
                } else {
                    Ok(None)
                }
            }
        } else if let Some(callback) = maybe_callback {
            let mut cmd_ret: Result<Option<XousString::<1024>>, xous::Error> = Ok(None);
            // first check and see if we have a callback registration; if not, just map to the last verb
            let verb = match self.common_env.cb_registrations.get(&(callback.body.id() as u32)) {
                Some(verb) => {
                    verb.to_str()
                },
                None => {
                    self.lastverb.to_str()
                }
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
            if verbfound {
                cmd_ret
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

/// extract the first token, as delimited by spaces
/// modifies the incoming line by removing the token and returning the remainder
/// returns the found token
pub fn tokenize(line: &mut XousString::<1024>) -> Option<XousString::<1024>> {
    let mut token = XousString::<1024>::new();
    let mut retline = XousString::<1024>::new();

    let lineiter = line.as_str().unwrap().chars();
    let mut foundspace = false;
    let mut foundrest = false;
    for ch in lineiter {
        if ch != ' ' && !foundspace {
            token.push(ch).unwrap();
        } else if foundspace && foundrest {
            retline.push(ch).unwrap();
        } else if foundspace && ch != ' ' {
            // handle case of multiple spaces in a row
            foundrest = true;
            retline.push(ch).unwrap();
        } else {
            foundspace = true;
            // consume the space
        }
    }
    line.clear();
    write!(line, "{}", retline.as_str().unwrap()).unwrap();
    if token.len() > 0 {
        Some(token)
    } else {
        None
    }
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


pub fn clock_not_set() -> bool {
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let seconds: u64 = since_the_epoch.as_secs();
    if seconds < DATE_2023 {
        true
    } else {
        false
    }
}
