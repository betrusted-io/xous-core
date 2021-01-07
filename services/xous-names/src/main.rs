#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate hash32_derive;

mod api;
use api::*;

// use heapless::String;
use heapless::FnvIndexMap;
use heapless::consts::*;

use log::{error, info};

const FAIL_TIMEOUT_MS: u64 = 100;

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
                info!("NS: registration request for {}", registration.name);
                if !name_table.contains_key(&registration.name) {
                    let new_sid = xous::create_server().expect("NS: create server failed, maybe OOM?");
                    name_table.insert(registration.name, new_sid).expect("NS: register name failure, maybe out of HashMap capacity?");
                    info!("NS: request successful, SID is {:?}", new_sid);
                    registration.sid = new_sid; // query: do we even need to return this?
                    registration.success = true;
                } else {
                    registration.success = false;
                    // compute the next interval, rounded to a multiple of FAIL_TIMEOUT_MS to reduce timing side channels
                    let target_time: u64 = ((ticktimer_server::elapsed_ms(ticktimer_conn).unwrap() / FAIL_TIMEOUT_MS) + 1) * FAIL_TIMEOUT_MS;
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
                /*
                info!("NS: Lookup request for {}", lookup.name);
                if let Ok(server_sid) = name_table.get(lookup) {
                    lookup.success = true;
                    let sender_pid = &envelope.sender.pid()?;
                    let connection_id = xous::connect_for_process(sender_pid, server_sid).expect("NS: can't broker connection");
                    lookup.cid = connection_id;
                } else {
                    lookup.success = false;
                    // no authenticate remedy currently supported, but we'd put that code somewhere around here eventually.
                }*/
            } else {
                error!("NS: unknown message ID received");
            }
        } else {
            error!("NS: couldn't convert opcode");
        }
    }
}
