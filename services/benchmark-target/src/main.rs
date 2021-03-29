#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use core::convert::TryFrom;

use log::{error, info};

use rkyv::archived_value_mut;
use core::pin::Pin;
use core::convert::TryInto;
use rkyv::{Serialize, Deserialize};
use rkyv::ser::Serializer;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    info!("BENCHTARGET: my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let bench_sid = xns.register_name(xous::names::SERVER_NAME_BENCHMARK).expect("BENCHTARGET: can't register server");
    info!("BENCHTARGET: registered with NS -- {:?}", bench_sid);

    loop {
        let envelope = xous::receive_message(bench_sid).unwrap();
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            match opcode {
                Opcode::TestScalar(val) => {
                    xous::return_scalar(envelope.sender, (val + 1) as usize)
                       .expect("BENCHTARGET: couldn't return TestScalar request");
                },
                _ => error!("BENCHTARGET: opcode not yet implemented"),
            }
        } else if let xous::Message::MutableBorrow(m) = &envelope.body {
            let mut buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let value = unsafe {
                archived_value_mut::<api::Opcode>(Pin::new(buf.as_mut()), m.id.try_into().unwrap())
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::TestMemory(reg) => {
                    use rkyv::Write;

                    let mut ret = TestStruct::new();
                    ret.challenge[0] = reg.challenge[0] + 1;

                    let retop = Opcode::TestMemory(ret);
                    let mut writer = rkyv::ser::serializers::BufferSerializer::new(buf);
                    let pos = writer.serialize_value(&retop).expect("couldn't archive return value");
                    unsafe{ rkyv::archived_value::<api::Opcode>(writer.into_inner().as_ref(), pos) };
                }
                _ => panic!("Invalid response from server -- corruption occurred"),
            };
        } else {
            error!("BENCHTARGET: couldn't convert opcode");
        }
    }
}
