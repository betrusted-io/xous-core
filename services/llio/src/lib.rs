#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;

use api::*;
use xous::{send_message, Error, CID};

pub fn allow_power_off(cid: CID, allow: bool) -> Result<(), xous::Error> {
    // note sense inversion on allow versus the opcode
    send_message(cid, api::Opcode::PowerSelf(!allow).into()).map(|_| ())
}
