use xous::{MessageEnvelope};
use xous_ipc::String as XousString;
use core::fmt::Write;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write as StdWrite, Error};
use std::path::PathBuf;

/// PDDB Dict for mtxcli keys
const MTXCLI_DICT: &str = "mtxcli";

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
#[allow(dead_code)] // there's more in the envornment right now than we need for the demo
pub struct CommonEnv {
    llio: llio::Llio,
    com: com::Com,
    codec: codec::Codec,
    ticktimer: ticktimer_server::Ticktimer,
    gam: gam::Gam,
    cb_registrations: HashMap::<u32, XousString::<256>>,
    trng: Trng,
    xns: xous_names::XousNames,
}
impl CommonEnv {

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), Error> {
        log::info!("set '{}' = '{}'", key, value);
        let mut keypath = PathBuf::new();
        keypath.push(MTXCLI_DICT);
        std::fs::create_dir_all(keypath.clone())?;
        keypath.push(key);
        File::create(keypath)?.write_all(value.as_bytes())?;
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<(), Error> {
        log::info!("unset '{}'", key);
        let mut keypath = PathBuf::new();
        keypath.push(MTXCLI_DICT);
        std::fs::create_dir_all(keypath.clone())?;
        keypath.push(key);
        if std::fs::metadata(keypath.clone()).is_ok() { // keypath exists
            std::fs::remove_file(keypath)?;
        }
        Ok(())
    }

    pub fn get(&mut self, key: &str) -> Result<Option<String>, Error> {
        let mut keypath = PathBuf::new();
        keypath.push(MTXCLI_DICT);
        std::fs::create_dir_all(keypath.clone())?;
        keypath.push(key);
        if let Ok(mut file)= File::open(keypath) {
            let mut value = String::new();
            file.read_to_string(&mut value)?;
            log::info!("get '{}' = '{}'", key, value);
            Ok(Some(value))
        } else {
            Ok(None)
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
mod help;      use help::*;
mod set;       use set::*;
mod status;    use status::*;
mod unset;     use unset::*;

pub struct CmdEnv {
    common_env: CommonEnv,
    lastverb: XousString::<256>,
    ///// 2. declare storage for your command here.
    get_cmd: Get,
    help_cmd: Help,
    set_cmd: Set,
    status_cmd: Status,
    unset_cmd: Unset,
}
impl CmdEnv {
    pub fn new(xns: &xous_names::XousNames) -> CmdEnv {
        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
        log::info!("creating CommonEnv");
        let common = CommonEnv {
            llio: llio::Llio::new(&xns),
            com: com::Com::new(&xns).expect("could't connect to COM"),
            codec: codec::Codec::new(&xns).expect("couldn't connect to CODEC"),
            ticktimer,
            gam: gam::Gam::new(&xns).expect("couldn't connect to GAM"),
            cb_registrations: HashMap::new(),
            trng: Trng::new(&xns).unwrap(),
            xns: xous_names::XousNames::new().unwrap(),
        };
        log::info!("done creating CommonEnv");
        CmdEnv {
            common_env: common,
            lastverb: XousString::<256>::new(),
            ///// 3. initialize your storage, by calling new()
            get_cmd: Get::new(&xns),
            help_cmd: Help::new(&xns),
            set_cmd: Set::new(&xns),
            status_cmd: Status::new(&xns),
            unset_cmd: Unset::new(&xns),
        }
    }

    pub fn dispatch(&mut self, maybe_cmdline: Option<&mut XousString::<1024>>, maybe_callback: Option<&MessageEnvelope>) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();

        let commands: &mut [& mut dyn ShellCmdApi] = &mut [
            ///// 4. add your command to this array, so that it can be looked up and dispatched
            &mut self.get_cmd,
            &mut self.help_cmd,
            &mut self.set_cmd,
            &mut self.status_cmd,
            &mut self.unset_cmd,
        ];

        if let Some(cmdline) = maybe_cmdline {
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
                    write!(ret, "SAY: {} {}", verb, cmdline.to_str()).unwrap();
                    Ok(Some(ret))
                }
            } else {
                Ok(None)
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
