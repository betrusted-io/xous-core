use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Set {
}
impl Set {
    pub fn new() -> Self {
        Set {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Set {
    cmd_api!(set);

    fn process(&mut self, args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
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
                                match env.set(key, value) {
                                    Ok(()) => {
                                        write!(ret, "set {}", key).unwrap();
                                    },
                                    Err(e) => {
                                        // write!(ret, "error setting key {}: {:?}", key, e).unwrap();
                                        log::error!("error setting key {}: {:?}", key, e);
                                    }
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
