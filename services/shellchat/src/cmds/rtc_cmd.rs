use std::time::SystemTime;

use chrono::offset::Utc;
use chrono::{DateTime, NaiveDateTime};
use xous_ipc::String;

use crate::{CommonEnv, ShellCmdApi};

#[derive(Debug)]
pub struct RtcCmd {}
impl RtcCmd {
    pub fn new(_xns: &xous_names::XousNames) -> Self { RtcCmd {} }
}
impl<'a> ShellCmdApi<'a> for RtcCmd {
    cmd_api!(rtc);

    fn process(
        &mut self,
        args: String<1024>,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "rtc options: utc local";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "utc" => {
                    let system_time = SystemTime::now();
                    let datetime: DateTime<Utc> = system_time.into();
                    write!(ret, "UTC time is {}", datetime.format("%m/%d/%Y %T")).unwrap();
                }
                "local" => {
                    let mut localtime = llio::LocalTime::new();
                    if let Some(timestamp) = localtime.get_local_time_ms() {
                        // we "say" UTC but actually local time is in whatever the local time is
                        let dt = chrono::DateTime::<Utc>::from_utc(
                            NaiveDateTime::from_timestamp(timestamp as i64 / 1000, 0),
                            chrono::offset::Utc,
                        );
                        let timestr = dt.format("%m/%d/%Y %T").to_string();
                        write!(ret, "Local time is {}", timestr).unwrap();
                        log::info!(
                            "{}RTC.LOCAL,{},{}",
                            xous::BOOKEND_START,
                            dt.format("%H,%M,%m,%d,%Y").to_string(),
                            xous::BOOKEND_END
                        );
                    } else {
                        write!(ret, "Local time has not been set up").unwrap();
                        log::info!("{}RTC.FAIL,{}", xous::BOOKEND_START, xous::BOOKEND_END);
                    }
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
        _msg: &xous::MessageEnvelope,
        _env: &mut CommonEnv,
    ) -> Result<Option<String<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        write!(ret, "{}", "Unrecognized callback to RTC").unwrap();
        Ok(Some(ret))
    }
}
