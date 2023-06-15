use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Get {
}
impl Get {
    pub fn new() -> Self {
        Get {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Get {
    cmd_api!(get);

    fn process(&mut self, args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.get.help", locales::LANG)).unwrap();
                }
                _ => {
                    match env.get(key) {
                        Ok(None) => {
                            write!(ret, "{} is UNSET", key).unwrap();
                        },
                        Ok(Some(value)) => {
                            write!(ret, "{}", value).unwrap();
                        }
                        Err(e) => {
                            log::error!("error getting key {}: {:?}", key, e);
                        }
                    }
                }
            }
        }

        Ok(Some(ret))
    }
}
