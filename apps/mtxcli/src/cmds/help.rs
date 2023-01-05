use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
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

    fn process(&mut self, args: XousString::<1024>, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(slashcmd) = tokens.next() {
            let cmd = if slashcmd.starts_with("/") {
                &slashcmd[1..]
            } else {
                slashcmd
            };
            match cmd {
                "get" => {
                    write!(ret, "{}", t!("mtxcli.get.help", xous::LANG)).unwrap();
                }
                "heap" => {
                    write!(ret, "{}", t!("mtxcli.heap.help", xous::LANG)).unwrap();
                }
                "help" => {
                    write!(ret, "{}", t!("mtxcli.help.help", xous::LANG)).unwrap();
                }
                "login" => {
                    write!(ret, "{}", t!("mtxcli.login.help", xous::LANG)).unwrap();
                }
                "logout" => {
                    write!(ret, "{}", t!("mtxcli.logout.help", xous::LANG)).unwrap();
                }
                "set" => {
                    write!(ret, "{}", t!("mtxcli.set.help", xous::LANG)).unwrap();
                }
                "status" => {
                    write!(ret, "{}", t!("mtxcli.status.help", xous::LANG)).unwrap();
                }
                "unset" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
                }
                "" => {
                    write!(ret, "{}\n", t!("mtxcli.help.overview", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.get.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.heap.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.help.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.login.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.logout.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.set.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.status.help", xous::LANG)).unwrap();
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
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
