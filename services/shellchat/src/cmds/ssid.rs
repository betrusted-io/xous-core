use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
use com::Com;
use ticktimer_server::Ticktimer;

use core::fmt::Write;
use xous::{Message, ScalarMessage, MessageEnvelope};
use num_traits::*;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum ScanResult {
    Ok,
    Timeout,
}

#[derive(Debug)]
pub struct Ssid {
    cb_id: Option<xous::CID>,
}
impl Ssid {
    pub fn new() -> Ssid {
        Ssid {
            cb_id: None,
        }
    }
}
impl<'a> ShellCmdApi<'a> for Ssid {
    cmd_api!(ssid);

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ssid [scan]";

        let mut tokens = args.as_str().unwrap().split(' ');
        if self.cb_id.is_none() {
            self.cb_id = Some(
                env.register_handler(String::<256>::from_str(self.verb()))
            );
        }

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "scan" => {
                    env.com.set_ssid_scanning(true).expect("couldn't turn on SSID scanning");
                    let _ = std::thread::spawn({
                        let cb_id = self.cb_id.expect("should have been initialized by this point");
                        move || {
                            let ticktimer = Ticktimer::new().expect("Couldn't connect to Ticktimer");
                            let xns = xous_names::XousNames::new().unwrap();
                            let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
                            let com = Com::new(&xns).unwrap();
                            let mut responded = false;
                            for _ in 0..15 {
                                if com.ssid_scan_updated().unwrap() {
                                    // just send a bogus message
                                    xous::send_message(callback_conn, Message::Scalar(ScalarMessage{
                                        id: cb_id as usize, arg1: ScanResult::Ok.to_usize().unwrap(), arg2: 0, arg3: 0, arg4: 0,
                                    })).unwrap();
                                    responded = true;
                                    break;
                                }
                                ticktimer.sleep_ms(2_000).unwrap();
                            }
                            if !responded {
                                xous::send_message(callback_conn, Message::Scalar(ScalarMessage{
                                    id: cb_id as usize, arg1: ScanResult::Timeout.to_usize().unwrap(), arg2: 0, arg3: 0, arg4: 0,
                                })).unwrap();
                            }
                        }
                    });
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

    fn callback(&mut self, msg: &MessageEnvelope, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        log::trace!("fetching SSID");
        xous::msg_scalar_unpack!(msg, arg1, _arg2, _arg3, _arg4, {
            match FromPrimitive::from_usize(arg1) {
                Some(ScanResult::Ok) => {
                    let ssid_list = env.com.ssid_fetch_as_list().unwrap();
                    write!(ret, "RSSI in dBm:\n").unwrap();
                    for (rssi, name) in ssid_list {
                        if name.len() > 0 {
                            write!(ret, "-{} {}\n", rssi, &name).unwrap();
                        }
                    }
                }
                Some(ScanResult::Timeout) => {
                    write!(ret, "SSID scan timed out").unwrap();
                }
                _ => log::error!("Unknown callback received by ssid scan")
            }

        });
        Ok(Some(ret))
    }
}
