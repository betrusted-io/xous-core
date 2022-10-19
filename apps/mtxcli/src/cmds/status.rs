use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
// use locales::t;

#[derive(Debug)]
pub struct Status {
}
impl Status {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Status {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Status {
    cmd_api!(status);

    fn process(&mut self, _args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        write!(ret, "status: not connected").unwrap();

        Ok(Some(ret))
    }
}
