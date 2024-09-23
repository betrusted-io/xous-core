use crate::cmds::*;
use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Status {}
impl Status {
    pub fn new() -> Self { Status {} }
}

impl<'a> ShellCmdApi<'a> for Status {
    cmd_api!(status);

    fn process(&mut self, _args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        if env.logged_in {
            env.scalar_async_msg(LOGGED_IN_ID);
        } else {
            env.scalar_async_msg(NOT_CONNECTED_ID);
        }
        Ok(None)
    }
}
