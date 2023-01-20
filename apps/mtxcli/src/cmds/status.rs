use crate::{ShellCmdApi,CommonEnv};
use crate::cmds::*;
use xous_ipc::String as XousString;

#[derive(Debug)]
pub struct Status {
}
impl Status {
    pub fn new() -> Self {
        Status {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Status {
    cmd_api!(status);

    fn process(&mut self, _args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        if env.logged_in {
            env.scalar_async_msg(LOGGED_IN_ID);
        } else {
            env.scalar_async_msg(NOT_CONNECTED_ID);
        }
        Ok(None)
    }
}
