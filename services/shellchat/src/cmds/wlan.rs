use core::fmt::Write;
use std::io::Write as PddbWrite;

use String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Wlan {
    current_ssid: Option<std::string::String>,
    current_pass: Option<std::string::String>,
}
impl Wlan {
    pub fn new() -> Self { Wlan { current_ssid: None, current_pass: None } }
}

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
    cmd_api!(wlan);

    // inserts boilerplate for command API

    fn process(
        &mut self,
        args: String,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let helpstring = "wlan [on] [off] [setssid ...] [setpass ...] [join] [leave] [status] [save] [known]";
        let mut show_help = false;

        let mut tokens = &args.split(' ');
        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "on" => {
                    let _ = match env.com.wlan_set_on() {
                        Ok(_) => write!(ret, "wlan on"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                    env.netmgr.connection_manager_wifi_on_and_run().unwrap();
                }
                "off" => {
                    env.netmgr.connection_manager_wifi_off_and_stop().unwrap();
                    let _ = match env.com.wlan_set_off() {
                        Ok(_) => write!(ret, "wlan off"),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "setssid" => {
                    // stop the connection manager from running if we're setting up an AP
                    env.netmgr.connection_manager_stop().unwrap();
                    let mut val = String::new();
                    join_tokens(&mut val, &mut tokens);
                    if val.len() == 0 {
                        let _ = write!(ret, "Error: SSID too short");
                    } else {
                        let _ = match env.com.wlan_set_ssid(val.as_str()) {
                            Ok(_) => {
                                self.current_ssid = Some(std::string::String::from(val.as_str()));
                                write!(
                                    ret,
                                    "wlan setssid {}.\nConnection manager paused during configuration.",
                                    val
                                )
                                .unwrap()
                            }
                            Err(_) => write!(ret, "Error: SSID too long for WF200").unwrap(),
                        };
                    }
                }
                "setpass" => {
                    env.netmgr.connection_manager_stop().unwrap();
                    let mut val = String::new();
                    join_tokens(&mut val, &mut tokens);
                    let _ = match env.com.wlan_set_pass(val.as_str()) {
                        Ok(_) => {
                            self.current_pass = Some(std::string::String::from(val.as_str()));
                            write!(
                                ret,
                                "wlan setpass {}.\nConnection manager paused during configuration.",
                                val
                            )
                            .unwrap()
                        }
                        Err(_) => write!(ret, "Error: passphrase too long for WF200").unwrap(),
                    };
                }
                "save" => {
                    let pddb = pddb::Pddb::new();
                    if let Some(ssid) = &self.current_ssid {
                        if let Some(pass) = &self.current_pass {
                            match pddb.get(
                                net::AP_DICT_NAME,
                                &ssid,
                                None,
                                true,
                                true,
                                Some(com::api::WF200_PASS_MAX_LEN),
                                Some(|| {}),
                            ) {
                                Ok(mut entry) => {
                                    match entry.write(&pass.as_bytes()) {
                                        Ok(len) => {
                                            if len != pass.len() {
                                                write!(
                                                    ret,
                                                    "PDDB wrote only {} of {} bytes of password",
                                                    len,
                                                    pass.len()
                                                )
                                                .unwrap();
                                            } else {
                                                // for now, we should always call flush at the end of a
                                                // routine; perhaps in the
                                                // future we'll have a timer that automatically syncs the pddb
                                                entry.flush().expect("couldn't sync pddb cache");
                                                write!(ret, "SSID/pass combo saved to PDDB.\nConnection manager started.").unwrap();
                                                // restart the connection manager now that the key combo has
                                                // been committed
                                                env.netmgr.connection_manager_run().unwrap();
                                            }
                                        }
                                        Err(e) => {
                                            write!(ret, "PDDB error storing key: {:?}", e).unwrap();
                                        }
                                    }
                                }
                                Err(e) => {
                                    write!(ret, "PDDB error creating key: {:?}", e).unwrap();
                                }
                            }
                        } else {
                            write!(ret, "No password currently set").unwrap();
                        }
                    } else {
                        write!(ret, "No SSID currently set").unwrap();
                    }
                }
                "known" => {
                    let pddb = pddb::Pddb::new();
                    match pddb.list_keys(net::AP_DICT_NAME, None) {
                        Ok(list) => {
                            write!(ret, "Saved network configs:\n").unwrap();
                            for item in list.iter() {
                                write!(ret, "- {}", item).ok(); // whatever, maybe we have too many?
                            }
                        }
                        Err(e) => {
                            write!(ret, "PDDB error accessing network configs: {:?}", e).unwrap();
                        }
                    }
                }
                "join" => {
                    let _ = match env.com.wlan_join() {
                        Ok(_) => {
                            write!(ret, "wlan join.\nConnection manager still paused, use `save` to resume connection manager.").unwrap();
                        }
                        Err(e) => write!(ret, "Error: {:?}", e).unwrap(),
                    };
                }
                "leave" => {
                    env.netmgr.connection_manager_stop().unwrap();
                    let _ = match env.com.wlan_leave() {
                        Ok(_) => write!(ret, "wlan leave.\nConnection manager stopped."),
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "status" => {
                    let _ = match env.com.wlan_status() {
                        Ok(msg) => {
                            log::info!(
                                "{}WLAN.STATUS,{:?},{}",
                                xous::BOOKEND_START,
                                std::net::IpAddr::from(msg.ipv4.addr),
                                xous::BOOKEND_END
                            );
                            write!(ret, "{:?}\n{:x?}", msg, msg)
                        }
                        Err(e) => write!(ret, "Error: {:?}", e),
                    };
                }
                "debug" => {
                    let debug = env.com.wlan_debug().expect("couldn't issue debug command");
                    write!(ret, "{:x?}", debug).unwrap();
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
fn join_tokens<'a>(buf: &mut String, tokens: impl Iterator<Item = &'a str>) {
    for (i, tok) in tokens.enumerate() {
        if i == 0 {
            write!(buf, "{}", tok).unwrap();
        } else {
            write!(buf, " {}", tok).unwrap();
        }
    }
}
