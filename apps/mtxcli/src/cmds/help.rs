use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Help {
}
impl Help {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Help {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Help {
    cmd_api!(help);

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        // write!(ret, "{}", t!("mtxcli.help.helpting", xous::LANG)).unwrap();

        if let Some(cmd) = tokens.next() {
            match cmd {
                "help" => {
                    write!(ret, "{}", t!("mtxcli.help.help", xous::LANG)).unwrap();
                }
                "quit" => {
                    write!(ret, "{}", t!("mtxcli.quit.help", xous::LANG)).unwrap();
                }
                "" => {
                    write!(ret, "{}\n", t!("mtxcli.help.overview", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.help.help", xous::LANG)).unwrap();
                    write!(ret, "{}", t!("mtxcli.quit.help", xous::LANG)).unwrap();
                }
                _ => {
                    write!(ret, "{}: {}",
                           t!("mtxcli.unknown.help", xous::LANG), cmd).unwrap();
                }
            }
        }

        Ok(Some(ret))
    }
}
