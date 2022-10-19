use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Get {
}
impl Get {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Get {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Get {
    cmd_api!(get);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.get.help", xous::LANG)).unwrap();
                }
                _ => {
                    let maybe_value = env.get(key);
                    if Ok(None) == maybe_value {
                        write!(ret, "{} is UNSET", key).unwrap();
                    } else if let Ok(Some(value)) = maybe_value {
                        // write!(ret, "{} = {}", key, value).unwrap();
                        write!(ret, "{}", value).unwrap();
                    } else {
                        write!(ret, "error getting key {}", key).unwrap();
                    }
                }
            }
        }

        Ok(Some(ret))
    }
}
