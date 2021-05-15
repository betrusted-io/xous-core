#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate hash32_derive;

mod api;
use api::*;

use num_traits::FromPrimitive;
use xous_ipc::{String, Buffer};

use log::{error, info};

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;

    pub struct D11cTimeout {
        d11t_csr: utralib::CSR<u32>,
    }
    impl D11cTimeout {
        pub fn new() -> Self {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::d11ctime::HW_D11CTIME_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map D11cTimeout CSR range");

            D11cTimeout {
                d11t_csr: CSR::new(csr.as_mut_ptr() as *mut u32),
            }
        }
        pub fn deterministic_busy_wait(&self) {
            let phase = self.d11t_csr.rf(utra::d11ctime::HEARTBEAT_BEAT);
            while phase == self.d11t_csr.rf(utra::d11ctime::HEARTBEAT_BEAT) {
                xous::yield_slice();
            }
        }
        pub fn hosted_delay(&self) {
            // this is a delay used in hosted mode to prevent threads from thrashing. We don't need this on
            // raw hardware because we aren't multi-core, and thus a yield_slice() will always schedule
            // a different process to run, whereas a yield_slice() on hosted/emulation can immediately return
            // and cause troubles for other processes.
        }
    }
}

#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct D11cTimeout {}
    impl D11cTimeout {
        pub fn new() -> Self {
            D11cTimeout {}
        }
        pub fn deterministic_busy_wait(&self) {
            // don't do anything for hosted mode
        }
        pub fn hosted_delay(&self) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/*
SlowMap is a stand-in implementation for a HashMap from the Heapless crate that has proven to be unsafe,
and leaking data between entries. It's called "SlowMap" because it's slow: accesses are O(N). That
being said, it's 100% safe, and xous-names accesses are once-in-a-blue-moon type of things, so
I'll take safety over speed in this case.

Eventually, we shall endeavor to remove Heapless entirely, once we have a `libstd` in place
and we can use heap-allocated Rust primitives...
*/
struct SlowMap {
    pub map: [Option<(XousServerName, xous::SID)>; 128],
}
impl SlowMap {
    pub fn new() -> Self {
        SlowMap {
            map: [None; 128],
        }
    }
    pub fn insert(&mut self, name: XousServerName, sid: xous::SID) -> Result<(), xous::Error> {
        let mut ok = false;
        for entry in self.map.iter_mut() {
            if entry.is_none() {
                *entry = Some((name, sid));
                ok = true;
                break;
            }
        }
        if !ok {
            Err(xous::Error::OutOfMemory)
        } else {
            Ok(())
        }
    }
    pub fn contains_key(&self, name: &XousServerName) -> bool {
        for maybe_entry in self.map.iter() {
            if let Some(entry) = maybe_entry {
                let (key, _value) = entry;
                if name == key {
                    return true
                }
            }
        }
        return false
    }
    pub fn get(&self, name: &XousServerName) -> Option<&xous::SID> {
        for maybe_entry in self.map.iter() {
            if let Some(entry) = maybe_entry {
                let (key, value) = entry;
                if name == key {
                    return Some(value)
                }
            }
        }
        None
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use implementation::*;
    let debug1 = false;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let name_server = xous::create_server_with_address(b"xous-name-server")
        .expect("Couldn't create xousnames-server");

    let d11ctimeout = D11cTimeout::new();

    // this limits the number of available servers to be requested to 128...!
    //let mut name_table = FnvIndexMap::<XousServerName, xous::SID, 128>::new();
    let mut name_table = SlowMap::new();

    info!("started");
    loop {
        let mut msg = xous::receive_message(name_server).unwrap();
        if debug1{info!("received message");}
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::Register) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let xous_string = buffer.to_original::<String::<64>, _>().unwrap();
                let name = XousServerName::from_str(xous_string.as_str().expect("couldn't convert server name to string"));

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
                    info!("request failed, waiting for deterministic timeout");
                    d11ctimeout.deterministic_busy_wait();
                    info!("deterministic timeout done");
                    response = api::Return::Failure
                }
                buffer.replace(response).expect("Register can't serialize return value");
            }
            Some(api::Opcode::Lookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let name_string = buffer.to_original::<String::<64>, _>().unwrap();
                let name = XousServerName::from_str(name_string.as_str().expect("couldn't convert server name to string"));
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
                            log::debug!(
                                "Can't find request '{}' in table, dumping table:",
                                name
                            );
                            for kv_tuple in name_table.map.iter() {
                                if let Some((key, val)) = kv_tuple {
                                    log::debug!("name: '{}', sid: '{:?}'", key, val);
                                }
                            }
                            d11ctimeout.hosted_delay();
                            response = api::Return::Failure
                        }
                    }
                } else {
                    log::debug!(
                        "Can't find request '{}' in table, dumping table:",
                        name
                    );
                    for kv_tuple in name_table.map.iter() {
                        if let Some((key, val)) = kv_tuple {
                            log::debug!("name: '{}', sid: '{:?}'", key, val);
                        }
                    }
                    // no authenticate remedy currently supported, but we'd put that code somewhere around here eventually.
                    let (c1, c2, c3, c4) = xous::create_server_id().unwrap().to_u32();
                    let auth_request = AuthenticateRequest {
                        name: String::<64>::from_str(name_string.as_str().expect("couldn't convert server name to string")),
                        pubkey_id: [0; 20], // placeholder
                        challenge: [c1, c2, c3, c4],
                    };
                    d11ctimeout.hosted_delay();
                    response = api::Return::AuthenticateRequest(auth_request) // this code just exists to exercise the return path
                }
                buffer.replace(response).expect("Lookup can't serialize return value");
            }
            Some(api::Opcode::AuthenticatedLookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let auth_lookup: AuthenticatedLookup = buffer.to_original().unwrap();
                info!("AuthenticatedLookup request {:?}", auth_lookup);
                error!("AuthenticatedLookup not yet implemented");
                unimplemented!("AuthenticatedLookup not yet implemented");
            }
            None => {error!("couldn't decode message: {:?}", msg); break}
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xous::destroy_server(name_server).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
