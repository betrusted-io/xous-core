mod echo;
use echo::*;

use xous::String;
use core::fmt::Write;

pub trait ShellCmdApi {
    // checks if the command matches the current verb in question
    fn matches(&self, verb: &str) -> bool;
    // called to process the command with the remainder of the string attached
    fn process(&self, rest: String::<1024>) -> Result<Option<String::<1024>>, xous::Error>;
    // returns my verb
    fn verb(&self) -> &str;
}

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

#[derive(Debug)]
pub struct CmdEnv {
}

impl CmdEnv {
    pub fn new() -> CmdEnv {
        CmdEnv {

        }
    }

    pub fn dispatch(&self, cmdline: &mut String::<1024>) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();

        let echo = Echo {};

        let maybe_verb = tokenize(cmdline);

        if let Some(verb_string) = maybe_verb {
            let verb = verb_string.to_str();
            if echo.matches(verb) {
                echo.process(*cmdline)
            } else {
                write!(ret, "Commands: ").unwrap();
                ret.append(echo.verb())?;

                Ok(Some(ret))
            }
        } else {
            Ok(None)
        }
    }
}