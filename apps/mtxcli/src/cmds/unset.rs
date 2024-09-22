use core::fmt::Write;

use locales::t;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Unset {}
impl Unset {
    pub fn new() -> Self { Unset {} }
}

impl<'a> ShellCmdApi<'a> for Unset {
    cmd_api!(unset);

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let mut tokens = &args.split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", locales::LANG)).unwrap();
                }
                _ => {
                    match env.unset(key) {
                        Ok(()) => {
                            write!(ret, "unset {}", key).unwrap();
                        }
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
