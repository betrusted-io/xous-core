use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct RtcCmd {
}
impl RtcCmd {
    pub fn new(_xns: &xous_names::XousNames) -> Self {
        RtcCmd {
        }
    }
}
impl<'a> ShellCmdApi<'a> for RtcCmd {
    cmd_api!(rtc);

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "rtc options: set, get";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "get" => {
                    write!(ret, "{}", "Requesting DateTime from RTC...").unwrap();
                    // TODO_RTC
                },
                "set" => {
                    // TODO_RTC
                },
                _ => {
                    write!(ret, "{}", helpstring).unwrap();
                }
            }

        } else {
            write!(ret, "{}", helpstring).unwrap();
        }
        Ok(Some(ret))
    }

    fn callback(&mut self, _msg: &xous::MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        write!(ret, "{}", "Unrecognized callback to RTC").unwrap();
        Ok(Some(ret))
    }

}
