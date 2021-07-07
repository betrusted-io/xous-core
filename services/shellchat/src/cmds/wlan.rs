use crate::{CommonEnv, ShellCmdApi};
use core::fmt::Write;
use xous_ipc::String;

#[derive(Debug)]
pub struct Wlan {}

/**
wlan shell command:
- on: if in off mode, reset WF200 and load firmware, otherwise NOP
- off: disconnect from AP (if joined) and put WF200 in low power standby
- setssid ...: set AP SSID to ... (... can include spaces)
- setpass ...: set AP password to ... (... can include spaces)
- join: if disconnected, connect by WPA2 personal with previously set SSID
        and password, otherwise NOP
- leave: if joined, disconnect from AP
- status: get wlan radio status (power state? connected? AP info?)
*/
impl<'a> ShellCmdApi<'a> for Wlan {
    cmd_api!(wlan); // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "wlan [on] [off] [setssid ...] [setpass ...] [join] [leave] [status]";
        let mut show_help = false;

        let mut tokens = args.as_str().unwrap().split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "on" => {
                    let _ = match env.com.wlan_set_on() {
                        Ok(_) => write!(ret, "wlan on"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "off" => {
                    let _ = match env.com.wlan_set_off() {
                        Ok(_) => write!(ret, "wlan off"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "setssid" => {
                    let mut val = String::<1024>::new();
                    join_tokens(&mut val, &mut tokens);
                    if val.len() == 0 {
                        let _ = write!(ret, "Error: SSID too short");
                    } else {
                        let _ = match env.com.wlan_set_ssid(&val) {
                            Ok(_) => write!(ret, "wlan setssid {}", val),
                            Err(_) => write!(ret, "Error: SSID too long for WF200"),
                        };
                    }
                }
                "setpass" => {
                    let mut val = String::<1024>::new();
                    join_tokens(&mut val, &mut tokens);
                    let _ = match env.com.wlan_set_pass(&val) {
                        Ok(_) => write!(ret, "wlan setpass {}", val),
                        Err(_) => write!(ret, "Error: passphrase too long for WF200"),
                    };
                }
                "join" => {
                    let _ = match env.com.wlan_join() {
                        Ok(_) => write!(ret, "wlan join"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "leave" => {
                    let _ = match env.com.wlan_leave() {
                        Ok(_) => write!(ret, "wlan leave"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "status" => {
                    let _ = match env.com.wlan_status() {
                        Ok(msg) => write!(ret, "{}", msg),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                _ => {
                    show_help = true;
                }
            }
        } else {
            show_help = true;
        }
        if show_help {
            let _ = write!(ret, "{}", helpstring);
        }
        Ok(Some(ret))
    }
}

/**
Join an iterator of string tokens with spaces.

This is intended to reverse the effect of .split(' ') in the context of a very simple
command parser. This is a lazy way to avoid building a parser for quoted strings, since
SSIDs or passwords might include spaces.
*/
fn join_tokens<'a>(buf: &mut String<1024>, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
