#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use xous::{CID, send_message};
use rkyv::ser::Serializer;

pub fn test_scalar(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    let response = send_message(cid, api::Opcode::TestScalar(testvar).into())?;
    if let xous::Result::Scalar1(r) = response {
        Ok(r as u32)
    } else {
        panic!("unexpected return value: {:#?}", response);
    }
}

pub fn test_memory(cid: CID, testvar: u32) -> Result<u32, xous::Error> {
    use rkyv::Write;
    let mut reg = TestStruct::new();
    reg.challenge[0] = testvar;

    let reg_opcode = api::Opcode::TestMemory(reg);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));

    let pos = writer.serialize_value(&reg_opcode).expect("couldn't archive test structure");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(cid, pos as u32).expect("test failure");

    let archived = unsafe {rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos) };
    match archived {
        rkyv::Archived::<api::Opcode>::TestMemory(result) => {
            Ok(result.challenge[0])
        },
        _ => panic!("mutable lend did not return a recognizable value")
    }
}
