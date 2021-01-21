#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use xous_names::api::Lookup;

use core::convert::TryFrom;

use log::{error, info};

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    let bench_sid = xous_names::register_name(xous::names::SERVER_NAME_BENCHMARK).expect("BENCHTARGET: can't register server");
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
            //let lookup: &mut Lookup = unsafe { &mut *(m.buf.as_mut_ptr() as *mut Lookup) };
            let lookup: &mut TestStruct = unsafe { &mut *(m.buf.as_mut_ptr() as *mut TestStruct) };
            lookup.challenge[0] = lookup.challenge[0] + 1;
        } else {
            error!("BENCHTARGET: couldn't convert opcode");
        }
    }
}
