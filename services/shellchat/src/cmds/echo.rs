use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Echo {}

impl<'a> ShellCmdApi<'a> for Echo {
    cmd_api!(echo);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        Ok(Some(args))
    }
}
