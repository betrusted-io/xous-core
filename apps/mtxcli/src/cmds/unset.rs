use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String as XousString;
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

    fn process(&mut self, args: XousString::<1024>, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
                }
                _ => {
                    match env.unset(key) {
                        Ok(()) => {
                            write!(ret, "unset {}", key).unwrap();
                        },
                        Err(e) => {
                            // NOTE: we current expect this error
                            // when unsetting a non existant key
                            write!(ret, "unset {} (did not exist)", key).unwrap();
                            log::error!("error unsetting key {}: {:?}", key, e);
                        }
                    }
                }
            }
        }

        Ok(Some(ret))
    }
}
