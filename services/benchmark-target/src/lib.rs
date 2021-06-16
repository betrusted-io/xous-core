#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use xous::{CID, send_message};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive};

pub fn test_scalar(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let response = send_message(cid,
        xous::Message::new_blocking_scalar(Opcode::TestScalar.to_usize().unwrap(), testvar as usize, 0, 0, 0))?;
    if let xous::Result::Scalar1(r) = response {
        Ok(r as u32)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn test_memory(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let mut reg = TestStruct::new();
    reg.challenge[0] = testvar;

    let mut buf = Buffer::into_buf(reg).or(Err(xous::Error::InternalError))?;
    buf.lend_mut(cid, Opcode::TestMemory.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

    let result = buf.as_flat::<TestStruct, _>().unwrap();
    Ok(result.challenge[0])
}

pub fn test_memory_send(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let mut reg = TestStruct::new();
    reg.challenge[0] = testvar;

    let mut buf = Buffer::into_buf(reg).or(Err(xous::Error::InternalError))?;
    buf.send(cid, Opcode::TestMemorySend.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
    Ok(testvar+2)
}
