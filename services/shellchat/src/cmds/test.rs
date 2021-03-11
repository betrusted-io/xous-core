use crate::{ShellCmdApi,CommonEnv};
use xous::String;

#[derive(Debug)]
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

impl<'a> ShellCmdApi<'a> for Test {
    cmd_api!(test);

    fn process(&mut self, _args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        self.state += 1;
        let mut ret = String::<1024>::new();
        write!(ret, "Test has run {} times.", self.state).unwrap();
        Ok(Some(ret))
    }
}