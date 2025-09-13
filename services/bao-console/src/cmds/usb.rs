use std::fmt::Write;

use String;
use usb_bao1x::UsbHid;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Usb {
    usb_dev: UsbHid,
}
impl Usb {
    pub fn new() -> Usb { Usb { usb_dev: UsbHid::new() } }
}

impl<'a> ShellCmdApi<'a> for Usb {
    cmd_api!(usb);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::new();
        let helpstring = "usb [send <string>]";

        let mut tokens = args.split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "send" => {
                    let mut val = String::new();
                    join_tokens(&mut val, &mut tokens);
                    match self.usb_dev.send_str(&val) {
                        Ok(n) => write!(ret, "Sent {} chars", n).unwrap(),
                        Err(_e) => write!(ret, "Can't send: are we connected to a host?").unwrap(),
                    }
                }
                "console" => {
                    // this will enable input injection mode
                    self.usb_dev.serial_console_input_injection();
                    write!(ret, "USB console connected.").ok();
                }
                "noconsole" => {
                    // this will disable any hooks (including the console input hook)
                    self.usb_dev.serial_clear_input_hooks();
                    write!(ret, "USB console disconnected.").ok();
                }
                "sersend" => {
                    let mut val = String::new();
                    join_tokens(&mut val, &mut tokens);
                    let sent = self.usb_dev.serial_send(val.as_bytes()).unwrap_or(0);
                    write!(ret, "Sent {} bytes", sent).ok();
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

fn join_tokens<'a>(buf: &mut String, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
