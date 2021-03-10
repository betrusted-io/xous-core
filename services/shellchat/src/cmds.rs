use xous::String;
use core::fmt::Write;

pub trait ShellCmdApi<'a> {
    // checks if the command matches the current verb in question
    fn matches(&self, verb: &str) -> bool;
    // called to process the command with the remainder of the string attached
    fn process(&mut self, rest: String::<1024>) -> Result<Option<String::<1024>>, xous::Error>;
    // returns my verb
    fn verb(&self) -> &'static str;
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

mod echo;
use echo::*;
mod test;
use test::*;

#[derive(Debug)]
pub struct CmdEnv {
    // placeholder for any environment variables relevant to interpreting commands
}

impl CmdEnv {
    pub fn new() -> CmdEnv {
        CmdEnv { }
    }

    pub fn dispatch(&self, cmdline: &mut String::<1024>) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();

        /*
           To add a new command:
             - ensure that the command implements the ShellCmdApi
             - mod/use the new command (above)
             - create a mutable version of the command's data structure
             - add the structure to the array all_commands below
        */
        let mut echo_cmd = Echo {};
        let mut test_cmd = Test::new();
        let all_commands: &mut [&mut dyn ShellCmdApi] = &mut [
            &mut echo_cmd,
            &mut test_cmd,
        ];

        let maybe_verb = tokenize(cmdline);

        let mut cmd_ret: Result<Option<String::<1024>>, xous::Error> = Ok(None);
        if let Some(verb_string) = maybe_verb {
            let verb = verb_string.to_str();

            // search through the list of commands linearly until one matches,
            // then run it.
            let mut match_found = false;
            for cmd in all_commands.iter_mut() {
                if cmd.matches(verb) {
                    match_found = true;
                    cmd_ret = cmd.process(*cmdline);
                };
            }

            // if none match, create a list of available commands
            if !match_found {
                let mut first = true;
                write!(ret, "Commands: ").unwrap();
                for cmd in all_commands.iter() {
                    if !first {
                        ret.append(", ")?;
                        first = false;
                    }
                    ret.append(cmd.verb())?;
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