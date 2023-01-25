use num_traits::*;
use std::{
    time::{SystemTime, UNIX_EPOCH},
};
use locales::t;
use chrono::{Utc, DateTime, NaiveDateTime};

// This file contains items that are used simultaneously within OpenSK and the `vault` app itself.
// These items need to be pulled in via both `lib` and `main` scopes.
// Vault-specific command to upload TOTP codes
pub const COMMAND_RESTORE_TOTP_CODES: u8 = 0x71;
pub const COMMAND_BACKUP_TOTP_CODES: u8 = 0x72;
pub const COMMAND_RESET_SESSION: u8 = 0x74;

pub const VAULT_PASSWORD_DICT: &'static str = "vault.passwords";
pub const VAULT_TOTP_DICT: &'static str = "vault.totp";
/// bytes to reserve for a key entry. Making this slightly larger saves on some churn as stuff gets updated
pub const VAULT_ALLOC_HINT: usize = 256;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum VaultOp {
    /// a line of text has arrived
    Line = 0, // make sure we occupy opcodes with discriminants < 1000, as the rest are used for callbacks
    /// incremental line of text
    IncrementalLine,
    /// redraw our UI
    Redraw,
    /// ignore dirty rectangles and redraw everything
    FullRedraw,
    /// reload the database (slow), and ignore dirty rectangles and redraw everything
    ReloadDbAndFullRedraw,
    /// change focus
    ChangeFocus,

    /// Partial menu
    MenuChangeFont,
    MenuDeleteStage1,
    MenuEditStage1,
    MenuAutotype,
    MenuReadoutMode,

    /// PDDB basis change
    BasisChange,

    /// Nop while waiting for prerequisites to be filled
    Nop,

    /// exit the application
    Quit,
}


pub fn atime_to_str(req_atime: u64) -> String {
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
pub fn utc_now() -> DateTime::<Utc> {
    let now =
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
    let naive = NaiveDateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos() as u32);
    DateTime::from_utc(naive, Utc)
}

/// app info format:
///
/// name: free form text string until newline
/// hash: app hash in hex string, lowercase
/// created: decimal number representing epoch of the creation date
/// last auth: decimal number representing epoch of the last auth time
pub struct AppInfo {
    pub name: String,
    pub id: [u8; 32],
    pub notes: String,
    pub ctime: u64,
    pub atime: u64,
    pub count: u64,
}

pub fn deserialize_app_info(descriptor: Vec::<u8>) -> Option::<AppInfo> {
    if let Ok(desc_str) = String::from_utf8(descriptor) {
        let mut appinfo = AppInfo {
            name: String::new(),
            notes: String::new(),
            id: [0u8; 32],
            ctime: 0,
            atime: 0,
            count: 0,
        };
        let lines = desc_str.split('\n');
        for line in lines {
            if let Some((tag, data)) = line.split_once(':') {
                match tag {
                    "name" => {
                        appinfo.name.push_str(data);
                    }
                    "notes" => appinfo.notes.push_str(data),
                    "id" => {
                        if let Ok(id) = hex::decode(data) {
                            appinfo.id.copy_from_slice(&id);
                        } else {
                            return None;
                        }
                    }
                    "ctime" => {
                        if let Ok(ctime) = u64::from_str_radix(data, 10) {
                            appinfo.ctime = ctime;
                        } else {
                            return None;
                        }
                    }
                    "atime" => {
                        if let Ok(atime) = u64::from_str_radix(data, 10) {
                            appinfo.atime = atime;
                        } else {
                            return None;
                        }
                    }
                    "count" => {
                        if let Ok(count) = u64::from_str_radix(data, 10) {
                            appinfo.count = count;
                        }
                        // count was added later, so, we don't fail if we don't see the record.
                    }
                    _ => {
                        log::warn!("unexpected tag {} encountered parsing app info, aborting", tag);
                        return None;
                    }
                }
            } else {
                log::trace!("invalid line skipped: {:?}", line);
            }
        }
        #[cfg(any(feature="precursor", feature="renode"))]
        if appinfo.name.len() > 0
        && appinfo.id != [0u8; 32]
        && appinfo.ctime != 0 { // atime can be 0 - indicates never used
            Some(appinfo)
        } else {
            None
        }
        #[cfg(not(target_os = "xous"))]
        if appinfo.name.len() > 0
        && appinfo.id != [0u8; 32] { // atime can be 0 - indicates never used. In hosted mode, ctime is 0.
            Some(appinfo)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn serialize_app_info<'a>(appinfo: &AppInfo) -> Vec::<u8> {
    format!("{}:{}\n{}:{}\n{}:{}\n{}:{}\n{}:{}\n",
        "name", appinfo.name,
        "id", hex::encode(appinfo.id),
        "ctime", appinfo.ctime,
        "atime", appinfo.atime,
        "count", appinfo.count,
    ).into_bytes()
}

pub fn basis_change() {
    log::info!("got basis change");
    xous::send_message(SELF_CONN.load(core::sync::atomic::Ordering::SeqCst),
        xous::Message::new_scalar(VaultOp::BasisChange.to_usize().unwrap(), 0, 0, 0, 0)
    ).unwrap();
}
pub static SELF_CONN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
