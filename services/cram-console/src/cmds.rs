use core::fmt::Write;
use std::collections::HashMap;

use cram_hal_service::trng;
#[cfg(feature = "shellperf")]
use utralib::generated::*;
use xous::MessageEnvelope;
use xous_ipc::String;

/////////////////////////// Common items to all commands
pub trait ShellCmdApi<'a> {
    // user implemented:
    // called to process the command with the remainder of the string attached
    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error>;
    // called to process incoming messages that may have been origniated by the most recently issued command
    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
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

/////////////////////////// Command shell integration
pub struct CommonEnv {
    ticktimer: ticktimer::Ticktimer,
    cb_registrations: HashMap<u32, String<256>>,
    trng: trng::Trng,
    #[allow(dead_code)]
    xns: xous_names::XousNames,
}
impl CommonEnv {
    #[allow(dead_code)]
    pub fn register_handler(&mut self, verb: String<256>) -> u32 {
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
}

/*
    To add a new command:
        0. ensure that the command implements the ShellCmdApi (above)
        1. mod/use the new command
        2. create an entry for the command's storage in the CmdEnv structure
        3. initialize the persistant storage here
        4. add it to the "commands" array in the dispatch() routine below

    Side note: if your command doesn't require persistent storage, you could,
    technically, generate the command dynamically every time it's called. Echo
    demonstrates this.
*/

///// 1. add your module here, and pull its namespace into the local crate
mod echo;
use echo::*;
mod ver;
use ver::*;
mod trng_cmd;
use trng_cmd::*;
mod test;
use test::*;
#[cfg(feature = "pddb")]
mod pddb_cmd;
#[cfg(feature = "pddb")]
use pddb_cmd::*;
mod usb;
use usb::*;
mod mbox;
use mbox::*;

pub struct CmdEnv {
    common_env: CommonEnv,
    lastverb: String<256>,
    ///// 2. declare storage for your command here.
    trng_cmd: TrngCmd,
    #[cfg(feature = "pddb")]
    pddb_cmd: PddbCmd,
    usb_cmd: Usb,
    mbox: Mbox,
}
impl CmdEnv {
    pub fn new(xns: &xous_names::XousNames) -> CmdEnv {
        let ticktimer = ticktimer::Ticktimer::new().expect("Couldn't connect to Ticktimer");
        // _ prefix allows us to leave the `mut` here without creating a warning.
        // the `mut` option is needed for some features.
        let mut _common = CommonEnv {
            ticktimer,
            cb_registrations: HashMap::new(),
            trng: trng::Trng::new(&xns).unwrap(),
            xns: xous_names::XousNames::new().unwrap(),
        };
        CmdEnv {
            common_env: _common,
            lastverb: String::<256>::new(),
            ///// 3. initialize your storage, by calling new()
            trng_cmd: {
                log::debug!("trng");
                TrngCmd::new()
            },
            #[cfg(feature = "pddb")]
            pddb_cmd: {
                log::debug!("pddb");
                PddbCmd::new(&xns)
            },
            usb_cmd: {
                log::debug!("usb");
                Usb::new()
            },
            mbox: Mbox::new(),
        }
    }

    pub fn dispatch(
        &mut self,
        maybe_cmdline: Option<&mut String<1024>>,
        maybe_callback: Option<&MessageEnvelope>,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();

        let mut echo_cmd = Echo {}; // this command has no persistent storage, so we can "create" it every time we call dispatch (but it's a zero-cost absraction so this doesn't actually create any instructions)
        let mut ver_cmd = Ver {};
        let mut console_cmd = Test {};
        let commands: &mut [&mut dyn ShellCmdApi] = &mut [
            ///// 4. add your command to this array, so that it can be looked up and dispatched
            &mut echo_cmd,
            &mut ver_cmd,
            &mut self.trng_cmd,
            &mut console_cmd,
            #[cfg(feature = "pddb")]
            &mut self.pddb_cmd,
            &mut self.usb_cmd,
            &mut self.mbox,
        ];

        if let Some(cmdline) = maybe_cmdline {
            let maybe_verb = tokenize(cmdline);

            let mut cmd_ret: Result<Option<String<1024>>, xous::Error> = Ok(None);
            if let Some(verb_string) = maybe_verb {
                let verb = verb_string.to_str();

                // search through the list of commands linearly until one matches,
                // then run it.
                let mut match_found = false;
                for cmd in commands.iter_mut() {
                    if cmd.matches(verb) {
                        match_found = true;
                        cmd_ret = cmd.process(*cmdline, &mut self.common_env);
                        self.lastverb.clear();
                        write!(self.lastverb, "{}", verb).expect("SHCH: couldn't record last verb");
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
                        ret.append(cmd.verb())?;
                        first = false;
                    }
                    Ok(Some(ret))
                } else {
                    cmd_ret
                }
            } else {
                Ok(None)
            }
        } else if let Some(callback) = maybe_callback {
            let mut cmd_ret: Result<Option<String<1024>>, xous::Error> = Ok(None);
            // first check and see if we have a callback registration; if not, just map to the last verb
            let verb = match self.common_env.cb_registrations.get(&(callback.body.id() as u32)) {
                Some(verb) => verb.to_str(),
                None => self.lastverb.to_str(),
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
/// note: we don't have split() because of nostd
pub fn tokenize(line: &mut String<1024>) -> Option<String<1024>> {
    let mut token = String::<1024>::new();
    let mut retline = String::<1024>::new();

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
    if token.len() > 0 { Some(token) } else { None }
}
