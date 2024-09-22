

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Logout {}
impl Logout {
    pub fn new() -> Self { Logout {} }
}

impl<'a> ShellCmdApi<'a> for Logout {
    cmd_api!(logout);

    fn process(
        &mut self,
        _args: String,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        env.logout();
        Ok(None)
    }
}
