use core::fmt::Write;

use locales::t;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Get {}
impl Get {
    pub fn new() -> Self { Get {} }
}

impl<'a> ShellCmdApi<'a> for Get {
    cmd_api!(get);

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let mut tokens = args.split(' ');

        if let Some(key) = tokens.next() {
            match key {
                "" => {
                    write!(ret, "{}", t!("mtxcli.get.help", locales::LANG)).unwrap();
                }
                _ => match env.get(key) {
                    Ok(None) => {
                        write!(ret, "{} is UNSET", key).unwrap();
                    }
                    Ok(Some(value)) => {
                        write!(ret, "{}", value).unwrap();
                    }
                    Err(e) => {
                        log::error!("error getting key {}: {:?}", key, e);
                    }
                },
            }
        }

        Ok(Some(ret))
    }
}
