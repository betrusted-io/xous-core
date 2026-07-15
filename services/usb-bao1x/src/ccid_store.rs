// SPDX-License-Identifier: Apache-2.0
//
//! Persist CCID provisioning strings in PDDB (opaque blobs; no verification here).

use std::io::{Read, Write};

use pddb::Pddb;
use usb_bao1x::ccid_framing::is_provisioned_marker;

pub(crate) const CCID_DICT: &str = "usb.ccid";
const KEY_PROVISIONED: &str = "provisioned";
const KEY_USER_LINE: &str = "user_pin_line";
const KEY_ADMIN_LINE: &str = "admin_pin_line";

pub(crate) fn is_ccid_provisioned(pddb: &Pddb) -> bool {
    match pddb.get(CCID_DICT, KEY_PROVISIONED, None, false, false, Some(32), None::<fn()>) {
        Ok(mut key) => {
            let mut buf = [0u8; 32];
            match key.read(&mut buf) {
                Ok(n) => is_provisioned_marker(&buf[..n]),
                _ => false,
            }
        }
        Err(_) => false,
    }
}

/// Write PIN lines + `OKV1` into PDDB. Not used by the USB path on Persona A CCID images
/// (no provisioning CDC); kept for offline / factory tooling that seeds PDDB before flash.
#[allow(dead_code)]
pub(crate) fn save_provisioned_pins(pddb: &Pddb, user_line: &[u8], admin_line: &[u8]) -> std::io::Result<()> {
    {
        let mut k = pddb.get(CCID_DICT, KEY_USER_LINE, None, true, true, Some(256), None::<fn()>)?;
        k.write_all(user_line)?;
        k.flush()?;
    }
    {
        let mut k = pddb.get(CCID_DICT, KEY_ADMIN_LINE, None, true, true, Some(256), None::<fn()>)?;
        k.write_all(admin_line)?;
        k.flush()?;
    }
    {
        let mut k = pddb.get(CCID_DICT, KEY_PROVISIONED, None, true, true, Some(32), None::<fn()>)?;
        k.write_all(b"OKV1")?;
        k.flush()?;
    }
    pddb.sync().ok();
    Ok(())
}
