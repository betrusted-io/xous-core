#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::FromPrimitive;
use xous::msg_blocking_scalar_unpack;
use xous_ipc::{Buffer, String};

use log::{error, info};

use std::collections::HashMap;

#[cfg(any(target_os = "none", target_os = "xous"))]
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

#[cfg(not(any(target_os = "none", target_os = "xous")))]
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
#[derive(Debug, Copy, Clone)]
struct Connection {
    pub sid: xous::SID,
    pub current_conns: u32, // number of unauthenticated (inherentely trusted) connections
    pub max_conns: Option<u32>, // if None, unlimited connections allowed
    pub allow_authenticate: bool,
    pub auth_conns: u32,         // number of authenticated connections
    pub token: Option<[u32; 4]>, // a random number that must be presented to allow for disconnection for single-connection servers
}
#[derive(Debug)]
struct CheckedHashMap {
    pub map: HashMap<XousServerName, Connection>,
}
impl CheckedHashMap {
    pub fn new() -> Self {
        CheckedHashMap {
            map: HashMap::new(),
        }
    }
    pub fn insert(
        &mut self,
        name: XousServerName,
        sid: xous::SID,
        max_conns: Option<u32>,
    ) -> Result<(), xous::Error> {
        let token = if max_conns == Some(1) {
            // for the special case of 1-connection servers, provision a one-time use token for disconnects
            Some(
                xous::create_server_id()
                    .expect("couldn't create token")
                    .to_array(),
            )
        } else {
            None
        };
        self.map.insert(
            name,
            Connection {
                sid,
                current_conns: 0,
                max_conns,
                allow_authenticate: false, // for now, we don't support authenticated connections
                auth_conns: 0,
                token,
            },
        );
        Ok(())
    }
    pub fn remove(&mut self, sid: xous::SID) -> Option<XousServerName> {
        // remove is expensive, because we have to do a full search for the sid, which is not our usual key
        // however, for security reasons, you have to let us know your sid (which is a secret) in order to delete
        // your entry; whereas the human-readable name is not at all a secret
        let mut removed_name: Option<XousServerName> = None;
        for (name, mapping) in self.map.iter_mut() {
            if mapping.sid == sid {
                removed_name = Some(*name);
                break;
            }
        }
        if let Some(name) = removed_name {
            self.map.remove(&name);
        }

        removed_name
    }
    pub fn contains_key(&self, name: &XousServerName) -> bool {
        self.map.contains_key(name)
    }
    pub fn connect(&mut self, name: &XousServerName) -> (Option<&xous::SID>, Option<[u32; 4]>) {
        let maybe_entry = self.map.get_mut(name);
        if let Some(entry) = maybe_entry {
            if Some(1) == entry.max_conns {
                // single-connection case
                if entry.current_conns < 1 {
                    (*entry).current_conns = 1;
                    return (Some(&entry.sid), entry.token);
                } else {
                    return (None, None);
                }
            }
            if let Some(max) = entry.max_conns {
                if entry.current_conns < max {
                    (*entry).current_conns += 1;
                    return (Some(&entry.sid), None);
                } else {
                    return (None, None);
                }
            } else {
                // unlimited connections allowed
                (*entry).current_conns += 1;
                return (Some(&entry.sid), None);
            }
        }
        (None, None)
    }
    pub fn trusted_init_done(&self) -> bool {
        let mut trusted_done = true;
        for (name, entry) in self.map.iter() {
            if let Some(max) = entry.max_conns {
                if max != entry.current_conns {
                    log::info!(
                        "server {} has {} conns but expects {}",
                        name,
                        entry.current_conns,
                        max
                    );
                    trusted_done = false;
                }
            }
        }
        trusted_done
    }
    // this function is slightly unsafe because we can't guarantee that the presenter of the SID
    // has actually discarded the SID. However, we don't currently anticipate using this path a lot.
    // If it does get used in security-critical routes, it should be refactored to regenerate the SID
    // and publish it to the server every time a disconnect is called, to ensure that after a disconnection
    // the caller can never talk to the server again.
    #[allow(dead_code)]
    pub fn disconnect(&mut self, sid: xous::SID) -> Option<XousServerName> {
        for (name, mapping) in self.map.iter_mut() {
            if mapping.sid == sid {
                if mapping.current_conns > 0 {
                    mapping.current_conns -= 1;
                }
                return Some(*name);
            }
        }
        None
    }
    // this is a safer version of disconnect. we track servers that allow exactly one connection at a time
    // and give them a one-time-use token that a connector can use to disconnect.
    pub fn disconnect_with_token(&mut self, name: &XousServerName, token: [u32; 4]) -> bool {
        if let Some(entry) = self.map.get_mut(name) {
            if let Some(old_token) = entry.token {
                if (token == old_token) && (entry.current_conns == 1) {
                    (*entry).current_conns = 0;
                    // generate the token -- we should never re-use these!
                    (*entry).token = Some(
                        xous::create_server_id()
                            .expect("couldn't create token")
                            .to_array(),
                    );
                    return true;
                }
            }
        }
        false
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use implementation::*;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let name_server = xous::create_server_with_address(b"xous-name-server")
        .expect("Couldn't create xousnames-server");

    let d11ctimeout = D11cTimeout::new();

    // this limits the number of available servers to be requested to 128...!
    //let mut name_table = FnvIndexMap::<XousServerName, xous::SID, 128>::new();
    let mut name_table = CheckedHashMap::new();

    info!("started");
    loop {
        let mut msg = xous::receive_message(name_server).unwrap();
        log::trace!("received message");
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::Register) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let registration = buffer.to_original::<Registration, _>().unwrap();
                let name = XousServerName::from_str(
                    registration
                        .name
                        .as_str()
                        .expect("couldn't convert server name to string"),
                );

                let response: api::Return;

                log::trace!("registration request for '{}'", name);
                if !name_table.contains_key(&name) {
                    let new_sid =
                        xous::create_server_id().expect("create server failed, maybe OOM?");
                    name_table
                        .insert(name, new_sid, registration.conn_limit)
                        .expect("register name failure, maybe out of HashMap capacity?");
                    log::trace!("request successful, SID is {:?}", new_sid);

                    response = api::Return::SID(new_sid.into());
                } else {
                    info!("request failed, waiting for deterministic timeout");
                    d11ctimeout.deterministic_busy_wait();
                    info!("deterministic timeout done");
                    response = api::Return::Failure
                }
                buffer
                    .replace(response)
                    .expect("Register can't serialize return value");
            }
            Some(api::Opcode::Unregister) => msg_blocking_scalar_unpack!(msg, s0, s1, s2, s3, {
                let gid = xous::SID::from_u32(s0 as u32, s1 as u32, s2 as u32, s3 as u32);
                if let Some(name) = name_table.remove(gid) {
                    info!("{} server has unregistered", name);
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    log::error!("couldn't unregister {:?}", gid);
                    log::error!("table: {:?}", name_table);
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(api::Opcode::Lookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let name_string = buffer.to_original::<String<64>, _>().unwrap();
                let name = XousServerName::from_str(
                    name_string
                        .as_str()
                        .expect("couldn't convert server name to string"),
                );
                log::trace!("Lookup request for '{}'", name);
                let response: api::Return;
                if let (Some(server_sid), token) = name_table.connect(&name) {
                    let sender_pid = msg
                        .sender
                        .pid()
                        .expect("can't extract sender PID on Lookup");
                    match xous::connect_for_process(sender_pid, *server_sid)
                        .expect("can't broker connection")
                    {
                        xous::Result::ConnectionID(connection_id) => {
                            log::trace!("lookup success, returning connection {}", connection_id);
                            response = api::Return::CID((connection_id, token))
                        }
                        _ => {
                            log::debug!("Can't find request '{}' in table, dumping table:", name);
                            for (_name, conn) in name_table.map.iter() {
                                log::debug!("{:?}", conn);
                            }
                            d11ctimeout.hosted_delay();
                            response = api::Return::Failure
                        }
                    }
                } else {
                    log::debug!("Can't find request '{}' in table, dumping table:", name);
                    for (_name, conn) in name_table.map.iter() {
                        log::debug!("{:?}", conn);
                    }
                    // no authenticate remedy currently supported, but we'd put that code somewhere around here eventually.
                    let (c1, c2, c3, c4) = xous::create_server_id().unwrap().to_u32();
                    let auth_request = AuthenticateRequest {
                        name: String::<64>::from_str(
                            name_string
                                .as_str()
                                .expect("couldn't convert server name to string"),
                        ),
                        pubkey_id: [0; 20], // placeholder
                        challenge: [c1, c2, c3, c4],
                    };
                    d11ctimeout.hosted_delay();
                    response = api::Return::AuthenticateRequest(auth_request) // this code just exists to exercise the return path
                }
                buffer
                    .replace(response)
                    .expect("Lookup can't serialize return value");
            }
            Some(api::Opcode::AuthenticatedLookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let auth_lookup: AuthenticatedLookup = buffer.to_original().unwrap();
                info!("AuthenticatedLookup request {:?}", auth_lookup);
                error!("AuthenticatedLookup not yet implemented");
                unimplemented!("AuthenticatedLookup not yet implemented");
            }
            Some(api::Opcode::TrustedInitDone) => {
                if name_table.trusted_init_done() {
                    xous::return_scalar(msg.sender, 1).expect("couldn't return trusted_init_done");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't return trusted_init_done");
                }
            }
            Some(api::Opcode::Disconnect) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let disconnect = buffer.to_original::<Disconnect, _>().unwrap();
                let name = XousServerName::from_str(disconnect.name.as_str().unwrap());
                let response = if name_table.disconnect_with_token(&name, disconnect.token) {
                    api::Return::Success
                } else {
                    api::Return::Failure
                };
                buffer.replace(response).expect("Can't return buffer");
            }
            None => {
                error!("couldn't decode message: {:?}", msg);
                break;
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xous::destroy_server(name_server).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0);
}
