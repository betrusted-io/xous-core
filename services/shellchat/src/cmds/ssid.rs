use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
use com::Com;
use ticktimer_server::Ticktimer;

use core::fmt::Write;

use xous::{Message, ScalarMessage, MessageEnvelope};
use core::sync::atomic::{AtomicBool, Ordering};
static CB_RUN: AtomicBool = AtomicBool::new(false);
pub fn callback_thread() {
    let ticktimer = Ticktimer::new().expect("Couldn't connect to Ticktimer");
    let xns = xous_names::XousNames::new().unwrap();
    let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
    let com = Com::new(&xns).unwrap();

    loop {
        if CB_RUN.load(Ordering::Relaxed) {
            log::trace!("checking ssid status");
            if com.ssid_scan_updated().unwrap() {
                log::trace!("initiating callback check");
                // just send a bogus message
                xous::send_message(callback_conn, Message::Scalar(ScalarMessage{
                    id: 0xdeadbeef, arg1: 0, arg2: 0, arg3: 0, arg4: 0,
                })).unwrap();
                CB_RUN.store(false, Ordering::Relaxed);
            }
            ticktimer.sleep_ms(2_000).unwrap();
        } else {
            ticktimer.sleep_ms(1_000).unwrap();
        }
    }
}

#[derive(Debug)]
pub struct Ssid {
}
impl Ssid {
    pub fn new() -> Ssid {
        xous::create_thread_0(callback_thread).expect("couldn't create callback generator thread");
        Ssid {
        }
    }
}
impl<'a> ShellCmdApi<'a> for Ssid {
    cmd_api!(ssid);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ssid [scan]";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "scan" => {
                    env.com.set_ssid_scanning(true).expect("couldn't turn on SSID scanning");
                    CB_RUN.store(true, Ordering::Relaxed);
                    write!(ret, "SSID scan initiated, please wait...").unwrap();
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

    fn callback(&mut self, _msg: &MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        log::trace!("fetching SSID");
        write!(ret, "{}", env.com.ssid_fetch_as_string().unwrap()).unwrap();
        Ok(Some(ret))
    }
}
