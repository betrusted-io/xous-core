use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Unset {
}
impl Unset {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        Unset {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Unset {
    cmd_api!(unset);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
                }
                _ => {
                    if Ok(None) == env.unset(key) {
                        write!(ret, "unset {}", key).unwrap();
                    } else {
                        write!(ret, "error unsetting key {}", key).unwrap();
                    }
                }
            }
        }

        Ok(Some(ret))
    }
}
