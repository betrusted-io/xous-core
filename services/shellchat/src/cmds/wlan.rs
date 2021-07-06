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
- show: show AP connection info (SSID, signal strength)
*/
impl<'a> ShellCmdApi<'a> for Wlan {
    cmd_api!(wlan); // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String<1024>,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "wlan [on] [off] [setssid ...] [setpass ...] [join] [leave] [show]";
        let mut show_help = false;

        let mut tokens = args.as_str().unwrap().split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "on" => {
                    //env.com.set_wlan_on().unwrap();
                    write!(ret, "wlan on").unwrap();
                }
                "off" => {
                    //env.com.set_wlan_off().unwrap();
                    write!(ret, "wlan off").unwrap();
                }
                "setssid" => {
                    let mut val = String::<1024>::new();
                    join_tokens(&mut val, &mut tokens);
                    //env.com.set_wlan_ssid(&val).unwrap();
                    write!(ret, "wlan setssid {}", val).unwrap();
                }
                "setpass" => {
                    let mut val = String::<1024>::new();
                    join_tokens(&mut val, &mut tokens);
                    //env.com.set_wlan_pass(&val).unwrap();
                    write!(ret, "wlan setpass {}", val).unwrap();
                }
                "join" => {
                    //env.com.wlan_join().unwrap();
                    write!(ret, "wlan join").unwrap();
                }
                "leave" => {
                    //env.com.wlan_leave().unwrap();
                    write!(ret, "wlan leave").unwrap();
                }
                "show" => {
                    //env.com.wlan_show().unwrap();
                    write!(ret, "wlan show").unwrap();
                }
                _ => {
                    show_help = true;
                }
            }
        } else {
            show_help = true;
        }
        if show_help {
            write!(ret, "{}", helpstring).unwrap();
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
