use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Quit {
}
impl Quit {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Quit {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Quit {
    cmd_api!(quit);

    fn process(&mut self, _args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();

        write!(ret, "{}", t!("mtxcli.quit.quitting", xous::LANG)).unwrap();
        env.gam.request_default_app().unwrap();
        Ok(Some(ret))
    }
}
