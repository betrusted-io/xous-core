use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct Keys {
}
impl Keys {
    pub fn new() -> Keys {
        Keys {}
    }
}

impl<'a> ShellCmdApi<'a> for Keys {
    cmd_api!(keys); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "keys options: usblock usbunlock";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "usblock" => {
                    env.llio.debug_usb(Some(true)).unwrap();
                    write!(ret, "USB debug port locked out; one word at 0x80000000 is disclosable via USB.").unwrap();
                }
                "usbunlock" => {
                    env.llio.debug_usb(Some(false)).unwrap();
                    write!(ret, "USB debug port unlocked: all secrets are readable via USB!").unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }

        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }
}
