use crate::{ShellCmdApi, CommonEnv};
use usb_device_xous::{UsbHid, UsbDeviceState};
use std::fmt::Write;

#[derive(Debug)]
pub struct Usb {
    usb_dev: UsbHid,
}
impl Usb {
    pub fn new() -> Usb {
        Usb {
            usb_dev: UsbHid::new(),
        }
    }
}


impl<'a> ShellCmdApi<'a> for Usb {
    cmd_api!(usb); // inserts boilerplate for command API

    fn process(&mut self, args: xous_ipc::String::<1024>, _env: &mut CommonEnv) -> Result<Option<xous_ipc::String::<1024>>, xous::Error> {
        let mut ret = xous_ipc::String::<1024>::new();
        let helpstring = "usb [send <string>] [status] [leds]";

        let mut tokens = args.as_str().unwrap().split(' ');

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
                "status" => {
                    match self.usb_dev.status() {
                        UsbDeviceState::Configured => write!(ret, "USB connected to host").unwrap(),
                        UsbDeviceState::Suspend => write!(ret, "Host put us in suspend").unwrap(),
                        _ => write!(ret, "Not connected to USB host").unwrap(),
                    }
                }
                "leds" => {
                    match self.usb_dev.get_led_state() {
                        Ok(leds) => write!(ret, "LEDs: {:?}", leds).unwrap(),
                        _ => write!(ret, "Not connected to USB host or other error").unwrap(),
                    }
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
