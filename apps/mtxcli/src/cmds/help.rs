use crate::{ShellCmdApi,CommonEnv,
            cmds::CLOCK_NOT_SET_ID};
use xous::{MessageEnvelope, Message,StringBuffer};
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
    fn callback(&mut self, msg: &MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<XousString::<1024>>, xous::Error> {
        match &msg.body {
            Message::Scalar(xous::ScalarMessage{id: _, arg1: _, arg2: _,
                                                arg3: _, arg4: async_msg_id}) => {
                if *async_msg_id == CLOCK_NOT_SET_ID {
                    let mut ret = XousString::<1024>::new();
                    let warning = t!("mtxcli.clock.warning", xous::LANG);
                    write!(ret, "{}", warning).unwrap();
                    log::warn!("{}", warning);
                    return Ok(Some(ret));
                }
            },
            Message::Move(mm) => {
                let str_buf = unsafe { StringBuffer::from_memory_message(mm) };
                let msg = str_buf.to_str();
                let mut ret = XousString::<1024>::new();
                write!(ret, "{}", msg).unwrap();
                // log::info!("async message \"{}\"", msg);
                return Ok(Some(ret));
            },
            _ => {
                log::error!("received unknown callback type: {:?}", msg)
            }
        }
        Ok(None)
    }
}
