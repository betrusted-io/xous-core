use std::fmt::Write;

use String;

use crate::{CommonEnv, ShellCmdApi};

pub struct EcUpdate {}
impl EcUpdate {
    pub fn new() -> Self { EcUpdate {} }
}

// this command is just a thin shim to shoot messages off to the EC update server, which is located
// in the Status crate.
impl<'a> ShellCmdApi<'a> for EcUpdate {
    cmd_api!(ecup);

    // inserts boilerplate for command API

    fn process(&mut self, args: String, env: &mut CommonEnv) -> Result<Option<String>, xous::Error> {
        let mut ret = String::new();
        let helpstring = "ecup [gw] [fw] [wf200] [auto]";

        log::debug!("ecup handling {}", &args);
        let mut tokens = &args.split(' ');
        let ecup_conn = env.xns.request_connection_blocking("__ECUP server__").unwrap();

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "fw" => {
                    let result = xous::send_message(
                        ecup_conn,
                        xous::Message::new_blocking_scalar(
                            1, // hard coded to match UpdateOp
                            0, 0, 0, 0,
                        ),
                    )
                    .unwrap();
                    env.ticktimer.sleep_ms(200).unwrap();
                    write!(ret, "EC firmware update: {:?}", result).unwrap();
                }
                "gw" => {
                    let result = xous::send_message(
                        ecup_conn,
                        xous::Message::new_blocking_scalar(
                            0, // hard coded to match UpdateOp
                            0, 0, 0, 0,
                        ),
                    )
                    .unwrap();
                    env.ticktimer.sleep_ms(200).unwrap();
                    write!(ret, "EC gateware update: {:?}", result).unwrap();
                }
                // note: "reset" has been moved to `ver ecreset`
                "wf200" => {
                    let result = xous::send_message(
                        ecup_conn,
                        xous::Message::new_blocking_scalar(
                            2, // hard coded to match UpdateOp
                            0, 0, 0, 0,
                        ),
                    )
                    .unwrap();
                    env.ticktimer.sleep_ms(200).unwrap();
                    write!(ret, "EC wf200 update: {:?}", result).unwrap();
                }
                "auto" => {
                    let result = xous::send_message(
                        ecup_conn,
                        xous::Message::new_blocking_scalar(
                            3, // hard coded to match UpdateOp
                            0, 0, 0, 0,
                        ),
                    )
                    .unwrap();
                    env.ticktimer.sleep_ms(200).unwrap();
                    write!(ret, "Full EC firmware update: {:?}", result).unwrap();
                }
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }
        }
        Ok(Some(ret))
    }
}
