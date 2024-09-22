

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Login {}
impl Login {
    pub fn new() -> Self { Login {} }
}

impl<'a> ShellCmdApi<'a> for Login {
    cmd_api!(login);

    fn process(
        &mut self,
        _args: String,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        env.login();
        Ok(None)
    }
}
