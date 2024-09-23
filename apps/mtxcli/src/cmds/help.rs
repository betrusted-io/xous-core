use core::fmt::Write;

use locales::t;
use xous::{Message, MessageEnvelope, StringBuffer};

use crate::cmds::*;
use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Help {}
impl Help {
    pub fn new() -> Self { Help {} }
}

impl<'a> ShellCmdApi<'a> for Help {
    cmd_api!(help);

    fn process(&mut self, args: String, _env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let mut tokens = args.split(' ');

        if let Some(slashcmd) = tokens.next() {
            let cmd = if slashcmd.starts_with("/") { &slashcmd[1..] } else { slashcmd };
            match cmd {
                "get" => {
                    write!(ret, "{}", t!("mtxcli.get.help", locales::LANG)).unwrap();
                }
                "heap" => {
                    write!(ret, "{}", t!("mtxcli.heap.help", locales::LANG)).unwrap();
                }
                "help" => {
                    write!(ret, "{}", t!("mtxcli.help.help", locales::LANG)).unwrap();
                }
                "login" => {
                    write!(ret, "{}", t!("mtxcli.login.help", locales::LANG)).unwrap();
                }
                "logout" => {
                    write!(ret, "{}", t!("mtxcli.logout.help", locales::LANG)).unwrap();
                }
                "set" => {
                    write!(ret, "{}", t!("mtxcli.set.help", locales::LANG)).unwrap();
                }
                "status" => {
                    write!(ret, "{}", t!("mtxcli.status.help", locales::LANG)).unwrap();
                }
                "unset" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", locales::LANG)).unwrap();
                }
                "" => {
                    write!(ret, "{}\n", t!("mtxcli.help.overview", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.get.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.heap.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.help.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.login.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.logout.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.set.help", locales::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.status.help", locales::LANG)).unwrap();
                    write!(ret, "{}", t!("mtxcli.unset.help", locales::LANG)).unwrap();
                }
                _ => {
                    write!(ret, "{}: {}", t!("mtxcli.unknown.help", locales::LANG), cmd).unwrap();
                }
            }
        }
        Ok(Some(ret))
    }

    // NOTE: the help callback is used to process async messages
    fn callback(
        &mut self,
        msg: &MessageEnvelope,
        env: &mut CommonEnv,
    ) -> Result<Option<String>, xous::Error> {
        match &msg.body {
            Message::Scalar(xous::ScalarMessage { id: _, arg1: _, arg2: _, arg3: _, arg4: async_msg_id }) => {
                let mut ret = String::new();
                let warning = match *async_msg_id {
                    CLOCK_NOT_SET_ID => {
                        t!("mtxcli.clock.warning", locales::LANG)
                    }
                    PDDB_NOT_MOUNTED_ID => {
                        t!("mtxcli.pddb.warning", locales::LANG)
                    }
                    WIFI_NOT_CONNECTED_ID => {
                        t!("mtxcli.wifi.warning", locales::LANG)
                    }
                    MTXCLI_INITIALIZED_ID => {
                        t!("mtxcli.initialized", locales::LANG)
                    }
                    WIFI_CONNECTED_ID => {
                        t!("mtxcli.wifi.connected", locales::LANG)
                    }
                    SET_USER_ID => {
                        t!("mtxcli.please.set.user", locales::LANG)
                    }
                    SET_PASSWORD_ID => {
                        t!("mtxcli.please.set.password", locales::LANG)
                    }
                    LOGGED_IN_ID => {
                        t!("mtxcli.logged.in", locales::LANG)
                    }
                    LOGIN_FAILED_ID => {
                        t!("mtxcli.login.failed", locales::LANG)
                    }
                    SET_ROOM_ID => {
                        t!("mtxcli.please.set.room", locales::LANG)
                    }
                    ROOMID_FAILED_ID => {
                        t!("mtxcli.roomid.failed", locales::LANG)
                    }
                    FILTER_FAILED_ID => {
                        t!("mtxcli.filter.failed", locales::LANG)
                    }
                    SET_SERVER_ID => {
                        t!("mtxcli.please.set.server", locales::LANG)
                    }
                    LOGGING_IN_ID => {
                        t!("mtxcli.logging.in", locales::LANG)
                    }
                    LOGGED_OUT_ID => {
                        t!("mtxcli.logged.out", locales::LANG)
                    }
                    NOT_CONNECTED_ID => {
                        t!("mtxcli.not.connected", locales::LANG)
                    }
                    FAILED_TO_SEND_ID => {
                        t!("mtxcli.send.failed", locales::LANG)
                    }
                    PLEASE_LOGIN_ID => {
                        t!("mtxcli.please.login", locales::LANG)
                    }
                    _ => "unknown async_msg_id",
                };
                write!(ret, "{}", warning).unwrap();
                log::warn!("{}", warning);
                return Ok(Some(ret));
            }
            Message::Move(mm) => {
                let str_buf = unsafe { StringBuffer::from_memory_message(mm) };
                let msg = str_buf.to_str();
                // log::info!("async message \"{}\"", msg);
                let mut ret = String::new();
                if msg.starts_with(SENTINEL) {
                    let msgv: Vec<&str> = msg.split(SENTINEL).collect();
                    if msgv.len() == 4 {
                        env.listen_over(msgv[1]);
                        if msgv[2].len() > 0 {
                            write!(ret, "{}", msgv[2]).unwrap();
                            return Ok(Some(ret));
                        } else {
                            return Ok(None);
                        }
                    } else {
                        log::info!("client_sync had an error");
                        env.listen_over(msgv[0]);
                        return Ok(None);
                    }
                } else {
                    write!(ret, "{}", msg).unwrap();
                    return Ok(Some(ret));
                }
            }
            _ => {
                log::error!("received unknown callback type: {:?}", msg)
            }
        }
        Ok(None)
    }
}
