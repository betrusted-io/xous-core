use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
use locales::t;

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
        let mut ret = XousString::<1024>::new();
        let mut text = String::new();
        text.push_str("status: ");
        if env.logged_in {
            text.push_str(t!("mtxcli.logged.in", xous::LANG));
        } else {
            text.push_str(t!("mtxcli.not.connected", xous::LANG));
        }
        env.prompt(&mut ret, &text);
        Ok(Some(ret))
    }
}
