#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate hash32_derive;

mod api;
use api::*;
use xous::buffer;

// use heapless::String;
use heapless::consts::*;
use heapless::FnvIndexMap;

use core::convert::TryInto;
use log::{error, info};

const FAIL_TIMEOUT_MS: u64 = 100;

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    log_server::init_wait().unwrap();
    info!("NS: my PID is {}", xous::process::id());

    let name_server = xous::create_server_with_address(b"xous-name-server")
        .expect("Couldn't create xousnames-server");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    // this limits the number of available servers to be requested to 128...!
    let mut name_table = FnvIndexMap::<_, _, U128>::new();

    info!("NS: started");
    use core::pin::Pin;
    use rkyv::archived_value_mut;

    loop {
        let envelope = xous::receive_message(name_server).unwrap();
        if debug1{info!("NS: received message");}
        if let xous::Message::MutableBorrow(m) = &envelope.body {
            let mut buf = unsafe { buffer::XousBuffer::from_memory_message(m) };
            let value = unsafe {
                archived_value_mut::<api::Request>(Pin::new(buf.as_mut()), m.id.try_into().unwrap())
            };
            let new_value = match &*value {
                rkyv::Archived::<api::Request>::Register(registration_name) => {
                    let name = XousServerName::from_str(registration_name.as_str());
                    if debug1{info!("NS: registration request for '{}'", name);}
                    if !name_table.contains_key(&name) {
                        let new_sid =
                            xous::create_server_id().expect("NS: create server failed, maybe OOM?");
                        name_table
                            .insert(name, new_sid)
                            .expect("NS: register name failure, maybe out of HashMap capacity?");
                        if debug1{info!("NS: request successful, SID is {:?}", new_sid);}
                        rkyv::Archived::<api::Request>::SID(new_sid.into())
                    } else {
                        // compute the next interval, rounded to a multiple of FAIL_TIMEOUT_MS to reduce timing side channels
                        let target_time: u64 = ((ticktimer.elapsed_ms()
                            .unwrap()
                            / FAIL_TIMEOUT_MS)
                            + 1)
                            * FAIL_TIMEOUT_MS;
                        info!("NS: request failed, waiting for deterministic timeout");
                        while ticktimer.elapsed_ms().unwrap() < target_time {
                            xous::yield_slice();
                        }
                        info!("NS: deterministic timeout done");
                        rkyv::Archived::<api::Request>::Failure
                    }
                }
                rkyv::Archived::<api::Request>::Lookup(lookup_name) => {
                    let name = XousServerName::from_str(lookup_name.as_str());
                    if debug1{info!("NS: Lookup request for '{}'", name);}
                    if let Some(server_sid) = name_table.get(&name) {
                        let sender_pid = envelope
                            .sender
                            .pid()
                            .expect("NS: can't extract sender PID on Lookup");
                        match xous::connect_for_process(sender_pid, *server_sid)
                            .expect("NS: can't broker connection")
                        {
                            xous::Result::ConnectionID(connection_id) => {
                                if debug1{info!("NS: lookup success, returning connection {}", connection_id);}
                                rkyv::Archived::<api::Request>::CID(connection_id)
                            }
                            _ => {
                                info!(
                                    "NS: Can't find request '{}' in table, dumping table:",
                                    name
                                );
                                for (key, val) in name_table.iter() {
                                    info!("NS: name: '{}', sid: '{:?}'", key, val);
                                }
                                rkyv::Archived::<api::Request>::Failure
                            }
                        }
                    } else {
                        info!(
                            "NS: Can't find request '{}' in table, dumping table:",
                            name
                        );
                        for (key, val) in name_table.iter() {
                            info!("NS: name: '{}', sid: '{:?}'", key, val);
                        }
                        // no authenticate remedy currently supported, but we'd put that code somewhere around here eventually.
                        rkyv::Archived::<api::Request>::Failure
                    }
                }
                _ => panic!("Invalid response from the server -- corruption occurred"),
            };
            unsafe { *value.get_unchecked_mut() = new_value };
        } else {
            error!("NS: couldn't convert opcode");
        }
    }
}
