use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;

#[derive(Debug)]
pub struct Login {
}
impl Login {
    pub fn new() -> Self {
        Login {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Login {
    cmd_api!(login);

    fn process(&mut self, _args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        env.login(&mut ret);
        Ok(Some(ret))
    }
}
