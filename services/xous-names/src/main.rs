#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate hash32_derive;

mod api;
use api::*;

use heapless::String;
use heapless::Vec;
use heapless::FnvIndexMap;
use heapless::consts::*;

use log::{error, info};

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    let name_server =
        xous::create_server_with_address(b"xous-name-server").expect("Couldn't create xousnames-server");

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    // this limits the number of available servers to be requested to 128...!
    let mut name_table = FnvIndexMap::<_,_,U128>::new();

    info!("NS: started");
    loop {
        let envelope = xous::receive_message(name_server).unwrap();
        info!("NS: received message");
        if let xous::Message::MutableBorrow(m) = &envelope.body {
            if m.id == ID_REGISTER_NAME {
                let registration: &mut Registration = unsafe {
                    &mut *(m.buf.as_mut_ptr() as *mut Registration)
                };
                let namevec: Vec<u8, U32> = Vec::from_slice(&registration.name.name).unwrap();
                let name: String<U32> = String::from_utf8(namevec).unwrap();
                info!("NS: registration request for {}", name);
                if !name_table.contains_key(&registration.name) {
                    let new_sid = xous::create_server().expect("NS: create server failed, maybe OOM?");
                    name_table.insert(registration.name, new_sid).expect("NS: register name failure, maybe out of HashMap capacity?");
                    info!("NS: request successful, SID is {:?}", new_sid);
                    registration.sid = new_sid; // query: do we even need to return this?
                    registration.success = true;
                } else {
                    registration.success = false;
                    // compute the next interval that is 0.5s away
                    let target_time: u64 = ((ticktimer_server::elapsed_ms(ticktimer_conn).unwrap() / 500) + 1) * 500;
                    info!("NS: request failed, waiting for deterministic timeout");
                    while ticktimer_server::elapsed_ms(ticktimer_conn).unwrap() < target_time {
                        xous::yield_slice();
                    }
                    info!("NS: deterministic timeout done");
                }
                // memory is automatically returend upon exit, no need for explicit return of memory
            } else if m.id == ID_LOOKUP_NAME {
                let lookup: &mut Lookup = unsafe {
                    &mut *(m.buf.as_mut_ptr() as *mut Lookup)
                };
                let namevec: Vec<u8, U32> = Vec::from_slice(&lookup.name.name).unwrap();
                let name: String<U32> = String::from_utf8(namevec).unwrap();
                info!("NS: Lookup request for {}", name);
                lookup.cid = 1337;
                lookup.success = true;
            } else {
                error!("NS: unknown message ID received");
            }
        } else {
            error!("NS: couldn't convert opcode");
        }
    }
}
