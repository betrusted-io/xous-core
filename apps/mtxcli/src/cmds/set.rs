use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Set {
}
impl Set {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Set {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Set {
    cmd_api!(set);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.set.help", xous::LANG)).unwrap();
                }
                _ => {
                    if let Some(value) = tokens.next() {
                        match value {
                            "" => {
                                write!(ret, "{}", t!("mtxcli.set.help", xous::LANG)).unwrap();
                            }
                            _ => {
                                if Ok(None) == env.set(key, value) {
                                    write!(ret, "set {} = {}", key, value).unwrap();
                                } else {
                                    write!(ret, "error setting key {}", key).unwrap();
                                }
                            }
                        }
                    } else {
                        write!(ret, "{}", t!("mtxcli.set.help", xous::LANG)).unwrap();
                    }
                }
            }
        }

        Ok(Some(ret))
    }
}
