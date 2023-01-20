use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;

#[derive(Debug)]
pub struct Logout {
}
impl Logout {
    pub fn new() -> Self {
        Logout {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Logout {
    cmd_api!(logout);

    fn process(&mut self, _args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        env.logout();
        Ok(None)
    }
}
