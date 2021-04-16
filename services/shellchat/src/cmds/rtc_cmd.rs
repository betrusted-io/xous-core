use crate::{ShellCmdApi,CommonEnv};
use xous_ipc::String;

use core::sync::atomic::{AtomicU32, Ordering};
static SHELLCONN: AtomicU32 = AtomicU32::new(0);
pub fn dt_callback(dt: rtc::DateTime) {
    let buf = xous_ipc::Buffer::into_buf(dt).or(Err(xous::Error::InternalError)).unwrap();
    log::trace!("SHELLCONN: {}", SHELLCONN.load(Ordering::Relaxed));
    buf.send(SHELLCONN.load(Ordering::Relaxed), 0xdead_beef).unwrap(); // send an "unknown ID" so it's routed to the callback handler
}

#[derive(Debug)]
pub struct RtcCmd {
    rtc: rtc::Rtc,
}
impl RtcCmd {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        let callback_conn = xns.request_connection_blocking(crate::SERVER_NAME_SHELLCHAT).unwrap();
        SHELLCONN.store(callback_conn, Ordering::Relaxed);
        let rtc = rtc::Rtc::new(&xns).unwrap();
        RtcCmd {
            rtc,
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
                    self.rtc.hook_rtc_callback(dt_callback).unwrap();
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
                        month = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
                    } else {
                        success = false;
                    }

                    if let Some(tok_str) = tokens.next() {
                        day = if let Ok(n) = tok_str.parse::<u8>() { n } else { success = false; 0 }
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
                        write!(ret, "{}", "usage: rtc set hh mm ss MM DD YY day\n'day' is three-day code, eg. mon tue").unwrap();
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
                        write!(ret, "Setting {}:{:02}:{:02}, {}/{}/{}, {:?}", hour, min, sec, month, day, year, weekday).unwrap();
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

    fn callback(&mut self, msg: &xous::MessageEnvelope, _env: &mut CommonEnv) -> Result<Option<String::<1024>>, xous::Error> {
        use core::fmt::Write;

        let buffer = unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
        let dt = buffer.to_original::<rtc::DateTime, _>().unwrap();

        let mut ret = String::<1024>::new();
        write!(ret, "{}:{:02}:{:02}, {}/{}/{}, {:?}", dt.hours, dt.minutes, dt.seconds, dt.months, dt.days, dt.years, dt.weekday).unwrap();

        self.rtc.unhook_rtc_callback().expect("can't unhook callback after completion");
        Ok(Some(ret))
    }

}
