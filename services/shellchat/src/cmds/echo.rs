use crate::ShellCmdApi;
use xous::String;

pub struct Echo {
}

const VERB: &str = "echo";

impl ShellCmdApi for Echo {
    fn matches(&self, verb: &str) -> bool {
        if verb == VERB {
            true
        } else {
            false
        }
    }

    fn process(&self, rest: String::<1024>) -> Result<Option<String::<1024>>, xous::Error> {
        Ok(Some(rest))
    }

    fn verb(&self) -> &str {
        VERB
    }
}