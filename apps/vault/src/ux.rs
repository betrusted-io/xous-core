pub(crate) mod fido;
pub(crate) use fido::*;
pub(crate) mod framework;
pub(crate) use framework::*;
pub(crate) mod icontray;
pub(crate) use icontray::*;

use locales::t;
use chrono::{Utc, DateTime, NaiveDateTime};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn atime_to_str(req_atime: u64) -> String {
    let mut request_str = String::new();
    if req_atime == 0 {
        request_str.push_str(&format!("{}{}",
            t!("vault.u2f.appinfo.last_authtime", xous::LANG),
            t!("vault.u2f.appinfo.never", xous::LANG)
        ));
    } else {
        let now = utc_now();
        let atime = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp(req_atime as i64, 0),
            Utc
        );
        if now.signed_duration_since(atime).num_days() > 1 {
            request_str.push_str(&format!("{}{}{}",
                t!("vault.u2f.appinfo.last_authtime", xous::LANG),
                now.signed_duration_since(atime).num_days(),
                t!("vault.u2f.appinfo.days_ago", xous::LANG),
            ));
        } else if now.signed_duration_since(atime).num_hours() > 1 {
            request_str.push_str(&format!("{}{}{}",
                t!("vault.u2f.appinfo.last_authtime", xous::LANG),
                now.signed_duration_since(atime).num_hours(),
                t!("vault.u2f.appinfo.hours_ago", xous::LANG),
            ));
        } else if now.signed_duration_since(atime).num_minutes() > 1 {
            request_str.push_str(&format!("{}{}{}",
                t!("vault.u2f.appinfo.last_authtime", xous::LANG),
                now.signed_duration_since(atime).num_minutes(),
                t!("vault.u2f.appinfo.minutes_ago", xous::LANG),
            ));
        } else {
            request_str.push_str(&format!("{}{}{}",
                t!("vault.u2f.appinfo.last_authtime", xous::LANG),
                now.signed_duration_since(atime).num_seconds(),
                t!("vault.u2f.appinfo.seconds_ago", xous::LANG),
            ));
        }
    }
    request_str
}

/// because we don't get Utc::now, as the crate checks your architecture and xous is not recognized as a valid target
pub(crate) fn utc_now() -> DateTime::<Utc> {
    let now =
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}