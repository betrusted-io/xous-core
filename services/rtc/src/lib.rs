#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};

pub fn get_u32(cid: CID) -> Result<u32, xous::Error> {
    let response = send_message(cid, api::Opcode::GetTrng(1).into())?;
    if let xous::Result::Scalar2(trng, _) = response {
        Ok(trng as u32)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn get_u64(cid: CID) -> Result<u64, xous::Error> {
    let response = send_message(cid, api::Opcode::GetTrng(2).into())?;
    if let xous::Result::Scalar2(lo, hi) = response {
        Ok( lo as u64 | ((hi as u64) << 32) )
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}
