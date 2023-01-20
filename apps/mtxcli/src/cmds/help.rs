use crate::{ShellCmdApi, CommonEnv};
use crate::cmds::*;
use xous::{MessageEnvelope, Message, StringBuffer};
use xous_ipc::String as XousString;
use core::fmt::Write;
use locales::t;

#[derive(Debug)]
pub struct Help {
}
impl Help {
    pub fn new() -> Self {
        Help {
        }
    }
}

impl<'a> ShellCmdApi<'a> for Help {
    cmd_api!(help);

    fn process(&mut self, args: XousString::<1024>, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        let mut ret = XousString::<1024>::new();
        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(slashcmd) = tokens.next() {
            let cmd = if slashcmd.starts_with("/") {
                &slashcmd[1..]
            } else {
                slashcmd
            };
            match cmd {
                "get" => {
                    write!(ret, "{}", t!("mtxcli.get.help", xous::LANG)).unwrap();
                }
                "heap" => {
                    write!(ret, "{}", t!("mtxcli.heap.help", xous::LANG)).unwrap();
                }
                "help" => {
                    write!(ret, "{}", t!("mtxcli.help.help", xous::LANG)).unwrap();
                }
                "login" => {
                    write!(ret, "{}", t!("mtxcli.login.help", xous::LANG)).unwrap();
                }
                "logout" => {
                    write!(ret, "{}", t!("mtxcli.logout.help", xous::LANG)).unwrap();
                }
                "set" => {
                    write!(ret, "{}", t!("mtxcli.set.help", xous::LANG)).unwrap();
                }
                "status" => {
                    write!(ret, "{}", t!("mtxcli.status.help", xous::LANG)).unwrap();
                }
                "unset" => {
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
                }
                "" => {
                    write!(ret, "{}\n", t!("mtxcli.help.overview", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.get.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.heap.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.help.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.login.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.logout.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.set.help", xous::LANG)).unwrap();
                    write!(ret, "{}\n", t!("mtxcli.status.help", xous::LANG)).unwrap();
                    write!(ret, "{}", t!("mtxcli.unset.help", xous::LANG)).unwrap();
                }
                _ => {
                    write!(ret, "{}: {}",
                           t!("mtxcli.unknown.help", xous::LANG), cmd).unwrap();
                }
            }
        }
        Ok(Some(ret))
    }

    // NOTE: the help callback is used to process async messages
    fn callback(&mut self, msg: &MessageEnvelope, env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        match &msg.body {
            Message::Scalar(xous::ScalarMessage{id: _, arg1: _, arg2: _,
                                                arg3: _, arg4: async_msg_id}) => {
                let mut ret = XousString::<1024>::new();
                let warning = match *async_msg_id {
                    CLOCK_NOT_SET_ID => {
                        t!("mtxcli.clock.warning", xous::LANG)
                    },
                    PDDB_NOT_MOUNTED_ID => {
                        t!("mtxcli.pddb.warning", xous::LANG)
                    },
                    WIFI_NOT_CONNECTED_ID => {
                        t!("mtxcli.wifi.warning", xous::LANG)
                    },
                    MTXCLI_INITIALIZED_ID => {
                        t!("mtxcli.initialized", xous::LANG)
                    },
                    WIFI_CONNECTED_ID => {
                        t!("mtxcli.wifi.connected", xous::LANG)
                    },
                    SET_USER_ID => {
                        t!("mtxcli.please.set.user", xous::LANG)
                    },
                    SET_PASSWORD_ID => {
                        t!("mtxcli.please.set.password", xous::LANG)
                    },
                    LOGGED_IN_ID => {
                        t!("mtxcli.logged.in", xous::LANG)
                    },
                    LOGIN_FAILED_ID => {
                        t!("mtxcli.login.failed", xous::LANG)
                    },
                    SET_ROOM_ID => {
                        t!("mtxcli.please.set.room", xous::LANG)
                    },
                    ROOMID_FAILED_ID => {
                        t!("mtxcli.roomid.failed", xous::LANG)
                    },
                    FILTER_FAILED_ID => {
                        t!("mtxcli.filter.failed", xous::LANG)
                    },
                    SET_SERVER_ID => {
                        t!("mtxcli.please.set.server", xous::LANG)
                    },
                    LOGGING_IN_ID => {
                        t!("mtxcli.logging.in", xous::LANG)
                    },
                    LOGGED_OUT_ID => {
                        t!("mtxcli.logged.out", xous::LANG)
                    },
                    NOT_CONNECTED_ID => {
                        t!("mtxcli.not.connected", xous::LANG)
                    },
                    FAILED_TO_SEND_ID => {
                        t!("mtxcli.send.failed", xous::LANG)
                    },
                    PLEASE_LOGIN_ID => {
                        t!("mtxcli.please.login", xous::LANG)
                    },
                    _ => {
                       "unknown async_msg_id"
                    },
                };
                write!(ret, "{}", warning).unwrap();
                log::warn!("{}", warning);
                return Ok(Some(ret));
            },
            Message::Move(mm) => {
                let str_buf = unsafe { StringBuffer::from_memory_message(mm) };
                let msg = str_buf.to_str();
                // log::info!("async message \"{}\"", msg);
                let mut ret = XousString::<1024>::new();
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
            },
            _ => {
                log::error!("received unknown callback type: {:?}", msg)
            }
        }
        Ok(None)
    }
}
