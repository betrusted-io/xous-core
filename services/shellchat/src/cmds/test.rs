use crate::ShellCmdApi;
use xous::String;

pub struct Test {
    state: u32
}
impl Test {
    pub fn new() -> Self {
        Test {
            state: 0
        }
    }
}

const VERB: &str = "test";

impl<'a> ShellCmdApi<'a> for Test {
    fn matches(&self, verb: &str) -> bool {
        if verb == VERB {
            true
        } else {
            false
        }
    }

    fn process(&mut self, _rest: String::<1024>) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        self.state += 1;
        let mut ret = String::<1024>::new();
        write!(ret, "Test has run {} times.", self.state).unwrap();
        Ok(Some(ret))
    }

    fn verb(&self) -> &'static str {
        VERB
    }
}