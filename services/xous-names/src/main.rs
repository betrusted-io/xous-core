#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate hash32_derive;

mod api;
use api::*;

// use heapless::String;
use heapless::consts::*;
use heapless::FnvIndexMap;

use num_traits::FromPrimitive;
use xous_ipc::{String, Buffer};

use log::{error, info};

const FAIL_TIMEOUT_MS: u64 = 100;


#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    log_server::init_wait().unwrap();
    info!("my PID is {}", xous::process::id());

    let name_server = xous::create_server_with_address(b"xous-name-server")
        .expect("Couldn't create xousnames-server");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    // this limits the number of available servers to be requested to 128...!
    let mut name_table = FnvIndexMap::<_, _, U128>::new();

    info!("started");
    loop {
        let mut msg = xous::receive_message(name_server).unwrap();
        if debug1{info!("received message");}
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::Register) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let xous_string = buffer.as_flat::<String::<64>, _>().unwrap();
                let name = XousServerName::from_str(xous_string.as_str());

                let response: api::Return;

                if debug1{info!("registration request for '{}'", name);}
                if !name_table.contains_key(&name) {
                    let new_sid =
                        xous::create_server_id().expect("create server failed, maybe OOM?");
                    name_table
                        .insert(name, new_sid)
                        .expect("register name failure, maybe out of HashMap capacity?");
                    if debug1{info!("request successful, SID is {:?}", new_sid);}

                    response = api::Return::SID(new_sid.into());
                } else {
                    // compute the next interval, rounded to a multiple of FAIL_TIMEOUT_MS to reduce timing side channels
                    let target_time: u64 = ((ticktimer.elapsed_ms()
                        / FAIL_TIMEOUT_MS)
                        + 1)
                        * FAIL_TIMEOUT_MS;
                    info!("request failed, waiting for deterministic timeout");
                    while ticktimer.elapsed_ms() < target_time {
                        xous::yield_slice();
                    }
                    info!("deterministic timeout done");
                    response = api::Return::Failure
                }
                buffer.replace(response).unwrap();
            }
            Some(api::Opcode::Lookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let name_string = buffer.as_flat::<String::<64>, _>().unwrap();
                let name = XousServerName::from_str(name_string.as_str());
                if debug1{info!("Lookup request for '{}'", name);}
                let response: api::Return;
                if let Some(server_sid) = name_table.get(&name) {
                    let sender_pid = msg
                        .sender
                        .pid()
                        .expect("can't extract sender PID on Lookup");
                    match xous::connect_for_process(sender_pid, *server_sid)
                        .expect("can't broker connection")
                    {
                        xous::Result::ConnectionID(connection_id) => {
                            if debug1{info!("lookup success, returning connection {}", connection_id);}
                            response = api::Return::CID(connection_id)
                        }
                        _ => {
                            info!(
                                "Can't find request '{}' in table, dumping table:",
                                name
                            );
                            for (key, val) in name_table.iter() {
                                info!("name: '{}', sid: '{:?}'", key, val);
                            }
                            response = api::Return::Failure
                        }
                    }
                } else {
                    info!(
                        "Can't find request '{}' in table, dumping table:",
                        name
                    );
                    for (key, val) in name_table.iter() {
                        info!("name: '{}', sid: '{:?}'", key, val);
                    }
                    // no authenticate remedy currently supported, but we'd put that code somewhere around here eventually.
                    let (c1, c2, c3, c4) = xous::create_server_id().unwrap().to_u32();
                    let auth_request = AuthenticateRequest {
                        name: String::<64>::from_str(name_string.as_str()),
                        pubkey_id: [0; 20], // placeholder
                        challenge: [c1, c2, c3, c4],
                    };
                    response = api::Return::AuthenticateRequest(auth_request) // this code just exists to exercise the return path
                }
                buffer.replace(response).unwrap();
            }
            Some(api::Opcode::AuthenticatedLookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let auth_lookup: AuthenticatedLookup = buffer.to_original().unwrap();
                info!("AuthenticatedLookup request {:?}", auth_lookup);
                error!("AuthenticatedLookup not yet implemented");
                unimplemented!("AuthenticatedLookup not yet implemented");
            }
            None => error!("couldn't decode message: {:?}", msg)
        }
    }
}
