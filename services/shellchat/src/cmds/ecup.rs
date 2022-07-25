use crate::{ShellCmdApi, CommonEnv};
use xous_ipc::String;
use std::fmt::Write;

pub struct EcUpdate {
}
impl EcUpdate {
    pub fn new() -> Self {
        EcUpdate {  }
    }
}

// this command is just a thin shim to shoot messages off to the EC update server, which is located
// in the Status crate.
impl<'a> ShellCmdApi<'a> for EcUpdate {
    cmd_api!(ecup); // inserts boilerplate for command API

    fn process(&mut self, args: String::<1024>, env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        let mut ret = String::<1024>::new();
        let helpstring = "ecup [gw] [fw] [wf200] [reset] [auto]";

        log::debug!("ecup handling {}", args.as_str().unwrap());
        let mut tokens = args.as_str().unwrap().split(' ');
        let ecup_conn = env.xns.request_connection_blocking("__ECUP server__").unwrap();

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "fw" => {
                    xous::send_message(ecup_conn,
                        xous::Message::new_blocking_scalar(
                            1, // hard coded to match UpdateOp
                            0, 0, 0, 0
                        )
                    ).unwrap();
                    write!(ret, "Starting EC firmware update").unwrap();
                }
                "gw" => {
                    xous::send_message(ecup_conn,
                        xous::Message::new_blocking_scalar(
                            0, // hard coded to match UpdateOp
                            0, 0, 0, 0
                        )
                    ).unwrap();
                    write!(ret, "Starting EC gateware update").unwrap();
                }
                // note: "reset" has been moved to `ver ecreset`
                "wf200" => {
                    xous::send_message(ecup_conn,
                        xous::Message::new_blocking_scalar(
                            2, // hard coded to match UpdateOp
                            0, 0, 0, 0
                        )
                    ).unwrap();
                    write!(ret, "Starting EC wf200 update").unwrap();
                }
                "auto" => {
                    xous::send_message(ecup_conn,
                        xous::Message::new_blocking_scalar(
                            3, // hard coded to match UpdateOp
                            0, 0, 0, 0
                        )
                    ).unwrap();
                    write!(ret, "Starting full EC firmware update").unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        }
        Ok(Some(ret))
    }

}
