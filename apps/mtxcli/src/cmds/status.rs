use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
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

    fn process(&mut self, _args: XousString::<1024>, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        write!(ret, "status: not connected").unwrap();

        Ok(Some(ret))
    }
}
