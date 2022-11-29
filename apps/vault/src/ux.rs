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
    let mut request_str = String::with_capacity(
        // avoid allocations to speed up this routine, it is in the inner loop of rendering lists of passwords
        t!("vault.u2f.appinfo.last_authtime", xous::LANG).len() +
        t!("vault.u2f.appinfo.seconds_ago", xous::LANG).len() +
        16 // space for the actual duration + some slop for translation
    );
    if req_atime == 0 {
        request_str.push_str(t!("vault.u2f.appinfo.last_authtime", xous::LANG));
        request_str.push_str(t!("vault.u2f.appinfo.never", xous::LANG));
    } else {
        let now = utc_now();
        let atime = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp(req_atime as i64, 0),
            Utc
        );
        // avoid format! macro, it is too slow.
        if now.signed_duration_since(atime).num_days() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", xous::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_days().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.days_ago", xous::LANG));
        } else if now.signed_duration_since(atime).num_hours() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", xous::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_hours().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.hours_ago", xous::LANG));
        } else if now.signed_duration_since(atime).num_minutes() > 1 {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", xous::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_minutes().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.minutes_ago", xous::LANG));
        } else {
            request_str.push_str(t!("vault.u2f.appinfo.last_authtime", xous::LANG));
            request_str.push_str(&now.signed_duration_since(atime).num_seconds().to_string());
            request_str.push_str(t!("vault.u2f.appinfo.seconds_ago", xous::LANG));
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