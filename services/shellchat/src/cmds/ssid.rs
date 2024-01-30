use core::fmt::Write;

use xous::MessageEnvelope;
use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct Ssid {
    cb_id: Option<xous::CID>,
}
impl Ssid {
    pub fn new() -> Ssid { Ssid { cb_id: None } }
}
impl<'a> ShellCmdApi<'a> for Ssid {
    cmd_api!(ssid);

    fn process(
        &mut self,
        args: String<1024>,
        env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ssid [scan]";

        let mut tokens = args.as_str().unwrap().split(' ');
        if self.cb_id.is_none() {
            self.cb_id = Some(env.register_handler(String::<256>::from_str(self.verb())));
        }

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "force" => {
                    env.com.set_ssid_scanning(true).unwrap();
                    write!(ret, "Forcing SSID scan. Warning: could put the connection manager in an inconsistent state.").unwrap();
                }
                "scan" => {
                    // SSID scanning is automatically initiated by the connection manager, so we don't
                    // initiate it explicitly, just report results
                    let (ssid_list, state) = env.netmgr.wifi_get_ssid_list().unwrap();
                    write!(ret, "RSSI reported in dBm:\n").unwrap();
                    for ssid in ssid_list {
                        if ssid.name.len() > 0 {
                            write!(ret, "-{} {}\n", ssid.rssi, &ssid.name.as_str().unwrap_or("UTF-8 error"))
                                .unwrap();
                        }
                    }
                    write!(ret, "Scan state: {:?}\n", state).unwrap();
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

    fn callback(
        &mut self,
        _msg: &MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        let ret = String::<1024>::new();
        Ok(Some(ret))
    }
}
