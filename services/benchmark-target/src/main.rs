#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
use xous::msg_blocking_scalar_unpack;

use log::{error, info};

use xous_ipc::Buffer;
use num_traits::{FromPrimitive};

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    info!("BENCHTARGET: my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let bench_sid = xns.register_name(api::SERVER_NAME_BENCHMARK, None).expect("BENCHTARGET: can't register server");
    info!("BENCHTARGET: registered with NS -- {:?}", bench_sid);

    let mut state: u32 = 0;
    loop {
        let mut envelope = xous::receive_message(bench_sid).unwrap();
        match FromPrimitive::from_usize(envelope.body.id()) {
            Some(Opcode::TestScalar) => msg_blocking_scalar_unpack!(envelope, val, _, _, _, {
                xous::return_scalar(envelope.sender, (val + 1) as usize)
                .expect("BENCHTARGET: couldn't return TestScalar request");
            }),
            Some(Opcode::TestMemory) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(envelope.body.memory_message_mut().unwrap()) };
                let reg = buffer.to_original::<TestStruct, _>().unwrap();

                let mut ret = TestStruct::new();
                ret.challenge[0] = reg.challenge[0] + 1;
                buffer.replace(ret).unwrap();
            },
            Some(Opcode::TestMemorySend) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(envelope.body.memory_message_mut().unwrap()) };
                let reg = buffer.to_original::<TestStruct, _>().unwrap();
                state += reg.challenge[0];
            }
            None => {error!("BENCHTARGET: couldn't convert opcode");}
        }
    }
}
