use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

#[derive(Debug)]
pub struct RtcCmd {
    rtc: rtc::Rtc,
}
impl RtcCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let mut rtc = rtc::Rtc::new(&xns).unwrap();
        RtcCmd {
            rtc: rtc,
        }
    }
}
impl<'a> ShellCmdApi<'a> for RtcCmd {
    cmd_api!(rtc);

    fn process(&mut self, args: String::<1024>, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;
        let mut ret = String::<1024>::new();
        let helpstring = "rtc options: set, get, hook";

        let mut tokens = args.as_str().unwrap().split(' ');

        if let Some(sub_cmd) = tokens.next() {
            match sub_cmd {
                "hook" => {
                    write!(ret, "{}", "Hooked RTC responder...").unwrap();
                    //self.rtc.hook_rtc_callback(dt_callback).unwrap();
                }
                "get" => {
                    write!(ret, "{}", "Requesting DateTime from RTC...").unwrap();
                    self.rtc.request_datetime().unwrap();
                },
                "set" => {
                    let mut success = true;
                    let mut hour: u8 = 0;
                    let mut min: u8 = 0;
                    let mut sec: u8 = 0;
                    let mut day: u8 = 0;
                    let mut month: u8 = 0;
                    let mut year: u8 = 0;
                    let mut weekday: rtc::Weekday = rtc::Weekday::Sunday;

                    if let Some(tok_str) = tokens.next() {
                        hour = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        min = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        sec = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        day = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        month = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        year = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        match tok_str {
                            "mon" => weekday = rtc::Weekday::Monday,
                            "tue" => weekday = rtc::Weekday::Tuesday,
                            "wed" => weekday = rtc::Weekday::Wednesday,
                            "thu" => weekday = rtc::Weekday::Thursday,
                            "fri" => weekday = rtc::Weekday::Friday,
                            "sat" => weekday = rtc::Weekday::Saturday,
                            "sun" => weekday = rtc::Weekday::Sunday,
                            _ => success = false,
                        }
                    } else {
                        success = false;
                    }
                    if !success {
                        write!(ret, "{}", "usage: rtc set hh mm ss DD MM YY day\n'day' is three-day code, eg. mon tue").unwrap();
                    } else {
                        let dt = rtc::DateTime {
                            seconds: sec,
                            minutes: min,
                            hours: hour,
                            days: day,
                            months: month,
                            years: year,
                            weekday,
                        };
                         write!(ret, "Setting {}:{}:{}, {}/{}/{}, {:?}", hour, min, sec, day, month, year, weekday).unwrap();
                        self.rtc.set_rtc(dt).unwrap();
                    }
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
}

static mut CB_CONN: Option<xous::CID> = None;
pub fn dt_callback(dt: rtc::DateTime) {
    if let Some(conn) = unsafe{CB_CONN} {
        log::info!("got datetime: {:?}", dt);
        let buf = xous_ipc::Buffer::into_buf(dt).or(Err(xous::Error::InternalError)).unwrap();
        buf.lend(conn, 0xdeadbeef).unwrap();
    } else {
        log::info!("callback not set, but got datetime: {:?}", dt);
    }
}
