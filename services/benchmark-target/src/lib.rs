#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};
use xous_names::api::Lookup;
use xous::ipc::Sendable;

pub fn test_scalar(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let response = send_message(cid, api::Opcode::TestScalar(testvar).into())?;
    if let xous::Result::Scalar1(r) = response {
        Ok(r as u32)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn test_memory(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let reg = Lookup::new();
    let mut sendable_reg = Sendable::new(reg).expect("can't create test structure");
    sendable_reg.challenge[0] = testvar;
    sendable_reg.lend_mut(cid, 0).expect("test failure");
    Ok(sendable_reg.challenge[0])
}