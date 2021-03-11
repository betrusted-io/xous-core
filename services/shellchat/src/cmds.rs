use xous::String;
use core::fmt::Write;
/////////////////////////// Common items to all commands
pub trait ShellCmdApi<'a> {
    // user implemented:
    // called to process the command with the remainder of the string attached
    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error>;

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

/////////////////////////// Command shell integration
#[derive(Debug)]
pub struct CommonEnv {
    llio: xous::CID,
    com: xous::CID,
    ticktimer: xous::CID,
    gam: xous::CID,
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
mod echo;     use echo::*;
mod test;     use test::*;
mod sleep;    use sleep::*;
mod sensors;  use sensors::*;

#[derive(Debug)]
pub struct CmdEnv {
    common_env: CommonEnv,
    ///// 2. declare storage for your command here.
    test_cmd: Test,
    sleep_cmd: Sleep,
    sensors_cmd: Sensors,
}
impl CmdEnv {
    pub fn new(gam: xous::CID) -> CmdEnv {
        let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
        CmdEnv {
            common_env: CommonEnv {
                llio: xous_names::request_connection_blocking(xous::names::SERVER_NAME_LLIO).expect("CMD: can't connect to LLIO"),
                com: xous_names::request_connection_blocking(xous::names::SERVER_NAME_COM).expect("CMD: can't connect to COM"),
                ticktimer: xous::connect(ticktimer_server_id).unwrap(),
                gam,
            },
            ///// 3. initialize your storage, by calling new()
            test_cmd: Test::new(),
            sleep_cmd: Sleep::new(),
            sensors_cmd: Sensors::new(),
        }
    }

    pub fn dispatch(&mut self, cmdline: &mut String::<1024>) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();

        let mut echo_cmd = Echo {}; // this command has no persistent storage, so we can "create" it every time we call dispatch (but it's a zero-cost absraction so this doesn't actually create any instructions)
        let commands: &mut [& mut dyn ShellCmdApi] = &mut [
            ///// 4. add your command to this array, so that it can be looked up and dispatched
            &mut echo_cmd,
            &mut self.test_cmd,
            &mut self.sleep_cmd,
            &mut self.sensors_cmd,
        ];

        let maybe_verb = tokenize(cmdline);

        let mut cmd_ret: Result<Option<String::<1024>>, xous::Error> = Ok(None);
        if let Some(verb_string) = maybe_verb {
            let verb = verb_string.to_str();

            // search through the list of commands linearly until one matches,
            // then run it.
            let mut match_found = false;
            for cmd in commands.iter_mut() {
                if cmd.matches(verb) {
                    match_found = true;
                    cmd_ret = cmd.process(*cmdline, &mut self.common_env);
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
    }
}

/// extract the first token, as delimited by spaces
/// modifies the incoming line by removing the token and returning the remainder
/// returns the found token
/// note: we don't have split() because of nostd
pub fn tokenize(line: &mut String::<1024>) -> Option<String::<1024>> {
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
    if token.len() > 0 {
        Some(token)
    } else {
        None
    }
}
