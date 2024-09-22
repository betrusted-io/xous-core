use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Backlight {}

impl<'a> ShellCmdApi<'a> for Backlight {
    cmd_api!(backlight);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "backlight [on] [off] [0-5]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            // note that the secondary backlight appears brighter, so generally, we want to set it to a lower
            // setting to save battery power
            match sub_cmd {
                "on" => {
                    env.com.set_backlight(255, 255).unwrap();
                    write!(ret, "backlight set to 100%").unwrap();
                }
                "off" => {
                    env.com.set_backlight(0, 0).unwrap();
                    write!(ret, "backlight turned off").unwrap();
                }
                "0" => {
                    env.com.set_backlight(0, 0).unwrap();
                    write!(ret, "backlight 0").unwrap();
                }
                "1" => {
                    env.com.set_backlight(32, 32).unwrap();
                    write!(ret, "backlight 1").unwrap();
                }
                "2" => {
                    env.com.set_backlight(96, 48).unwrap();
                    write!(ret, "backlight 2").unwrap();
                }
                "3" => {
                    env.com.set_backlight(128, 64).unwrap();
                    write!(ret, "backlight 3").unwrap();
                }
                "4" => {
                    env.com.set_backlight(196, 96).unwrap();
                    write!(ret, "backlight 4").unwrap();
                }
                "5" => {
                    env.com.set_backlight(255, 128).unwrap();
                    write!(ret, "backlight 5").unwrap();
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
