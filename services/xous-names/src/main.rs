#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use std::collections::HashMap;

use log::{error, info};
use num_traits::FromPrimitive;
use xous::{msg_blocking_scalar_unpack, MessageEnvelope};
use xous_api_names::api::*;
use xous_api_names::*;
use xous_ipc::{Buffer, String};

#[derive(PartialEq)]
#[repr(C)]
enum ConnectError {
    /// The length of the memory buffer was invalid
    InvalidMemoryBuffer = 1,

    /// The `connect_for_process()` call failed
    KernelConnectFailure = 2,

    /// The specified nameserver string was not UTF-8
    InvalidString = 3,

    /// The message was not a mutable memory message
    InvalidMessageType = 4,

    /// The server does not currently exist, and a blocking request was made
    ServerNotFound = 5,
}

#[derive(PartialEq)]
#[repr(C)]
enum ConnectSuccess {
    /// The server connection was successfully made
    Connected(xous::CID /* Connection ID */, Option<[u32; 4]> /* Disconnection token */),

    /// There is no server with that name -- block this message
    Wait,
    // /// The client needs to make an authentication request
    // AuthenticationRequest
}

#[cfg(any(feature = "precursor", feature = "renode", feature = "cramium-fpga", feature = "cramium-soc"))]
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

            D11cTimeout { d11t_csr: CSR::new(csr.as_mut_ptr() as *mut u32) }
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

#[cfg(any(not(target_os = "xous"),
    not(any(feature="precursor", feature="renode", feature="cramium-fpga", feature="cramium-soc", not(target_os = "xous"))) // default to pass crates.io build
))]
mod implementation {
    pub struct D11cTimeout {}
    impl D11cTimeout {
        pub fn new() -> Self { D11cTimeout {} }

        pub fn deterministic_busy_wait(&self) {
            // don't do anything for hosted mode
        }

        pub fn hosted_delay(&self) { std::thread::sleep(std::time::Duration::from_millis(10)); }
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
    pub current_conns: u32, // number of unauthenticated (inherently trusted) connections
    pub max_conns: Option<u32>, // if None, unlimited connections allowed
    pub _allow_authenticate: bool,
    pub _auth_conns: u32,        // number of authenticated connections
    pub token: Option<[u32; 4]>, // a random number that must be presented to allow for disconnection
}
#[derive(Debug)]
struct CheckedHashMap {
    pub map: HashMap<XousServerName, Connection>,
}
impl CheckedHashMap {
    pub fn new() -> Self { CheckedHashMap { map: HashMap::new() } }

    pub fn insert(
        &mut self,
        name: XousServerName,
        sid: xous::SID,
        max_conns: Option<u32>,
    ) -> Result<(), xous::Error> {
        let token =
            // for use with 1-connection servers, provision a one-time use token for disconnects
            // it will be returned for multi-connection servers as well, but it doesn't have a clear
            // semantic meaning with multiple connections. However, this exists in particular to
            // allow clean connect/disconnect in the special case of 1-connection servers that
            // can be swapped out (such as plugins for IME predictions)
            Some(
                xous::create_server_id()
                    .expect("couldn't create token")
                    .to_array(),
            );
        self.map.insert(
            name,
            Connection {
                sid,
                current_conns: 0,
                max_conns,
                _allow_authenticate: false, // for now, we don't support authenticated connections
                _auth_conns: 0,
                token,
            },
        );
        Ok(())
    }

    pub fn remove(&mut self, sid: xous::SID) -> Option<XousServerName> {
        // remove is expensive, because we have to do a full search for the sid, which is not our usual key
        // however, for security reasons, you have to let us know your sid (which is a secret) in order to
        // delete your entry; whereas the human-readable name is not at all a secret
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

    pub fn contains_key(&self, name: &XousServerName) -> bool { self.map.contains_key(name) }

    pub fn connect(&mut self, name: &XousServerName) -> (Option<xous::SID>, Option<[u32; 4]>) {
        if let Some(entry) = self.map.get_mut(name) {
            match entry.max_conns {
                // single-connection case
                Some(1) => {
                    if entry.current_conns < 1 {
                        (*entry).current_conns = 1;
                        (Some(entry.sid), entry.token)
                    } else {
                        (None, None)
                    }
                }
                Some(max) => {
                    if entry.current_conns < max {
                        (*entry).current_conns += 1;
                        (Some(entry.sid), entry.token)
                    } else {
                        log::warn!("Attempt to connect, but no connections available: {:?}", name.to_str());
                        (None, None)
                    }
                }
                _ => {
                    // unlimited connections allowed
                    (*entry).current_conns += 1;
                    // previously, this did not return an entry.token, but we now do
                    // because we had to loosen the restriction on the count of connections to
                    // the IME plugins -- because by essence, a disconnected IME plugin does
                    // not have its connection table full, and therefore, this would disallow
                    // root key operations. However, we still want some control over who
                    // is allowed to initiate the disconnect, so, we've moved the access
                    // control to the server itself, thus allowing a permissive policy inside
                    // xous-names.
                    (Some(entry.sid), entry.token)
                }
            }
        } else {
            (None, None)
        }
    }

    pub fn trusted_init_done(&self) -> bool {
        let mut trusted_done = true;
        for (name, entry) in self.map.iter() {
            if let Some(max) = entry.max_conns {
                if max != entry.current_conns {
                    log::info!("server {} has {} conns but expects {}", name, entry.current_conns, max);
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
                if token == old_token {
                    if entry.current_conns != 1 {
                        log::warn!(
                            "disconnect with token used on system with more than one client; this will cause other disconnects to fail"
                        );
                    }
                    (*entry).current_conns = 0;
                    // generate the token -- we should never re-use these!
                    (*entry).token =
                        Some(xous::create_server_id().expect("couldn't create token").to_array());
                    return true;
                }
            }
        }
        false
    }
}

fn name_from_msg(env: &MessageEnvelope) -> Result<XousServerName, ConnectError> {
    let msg = env.body.memory_message().ok_or(ConnectError::InvalidMessageType)?;
    let valid_bytes = msg.valid.map(|v| v.get()).unwrap_or_else(|| msg.buf.len());
    if valid_bytes > msg.buf.len() {
        log::error!("valid bytes exceeded entire buffer length");
        return Err(ConnectError::InvalidMemoryBuffer);
    }
    // Safe because we've already validated that it's a valid range
    let str_slice = unsafe { core::slice::from_raw_parts(msg.buf.as_ptr(), valid_bytes) };
    let name_string = core::str::from_utf8(str_slice).map_err(|_| ConnectError::InvalidString)?;

    Ok(XousServerName::from_str(name_string))
}

/// Connect to the server named in the message. If the server exists, attempt the connection
/// and return either the connection ID or an error.
///
/// If the server does not exist, return `Ok(None)`
fn blocking_connect(
    env: &mut MessageEnvelope,
    name_table: &mut CheckedHashMap,
) -> Result<ConnectSuccess, ConnectError> {
    let name = name_from_msg(env)?;
    let sender_pid = env.sender.pid().expect("kernel provided us a PID of None");
    log::trace!("BlockingConnect request for '{}' for process {:?}", name, sender_pid);

    // If the server already exists, attempt to make the connection. The connection can
    // only succeed if the server is in the name_table.
    if let (Some(server_sid), token) = name_table.connect(&name) {
        log::trace!(
            "Found entry in the table (sid: {:?}, token: {:?}) -- attempting to call connect_for_process()",
            server_sid,
            token
        );
        let result = xous::connect_for_process(sender_pid, server_sid);
        if let Ok(xous::Result::ConnectionID(connection_id)) = result {
            log::trace!(
                "Connected to '{}' for process {:?} with CID {} and disconnect token of {:?}",
                name,
                sender_pid,
                connection_id,
                token
            );
            return Ok(ConnectSuccess::Connected(connection_id, token));
        } else {
            log::error!("error when making connection, perhaps the server crashed? {:?}", result);

            // The server connection process failed inside the kernel for one reason or
            // another, so remove the entry from the `name_table` and return an error
            name_table.disconnect(server_sid);
            return Err(ConnectError::KernelConnectFailure);
        }
    }

    // There is no connection, so block the sender
    log::trace!("No server currently registered to '{}', blocking...", name);
    Ok(ConnectSuccess::Wait)
}

fn respond_connect_error(mut msg: MessageEnvelope, result: ConnectError) {
    let mem = msg.body.memory_message_mut().unwrap();
    let s = unsafe { core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr() as *mut u32, mem.buf.len() / 4) };
    s[0] = 1;
    s[1] = result as u32;
    mem.valid = None;
    mem.offset = None;
}

fn respond_connect_success(mut msg: MessageEnvelope, cid: xous::CID, disc: Option<[u32; 4]>) {
    let mem = msg.body.memory_message_mut().unwrap();
    let s = unsafe { core::slice::from_raw_parts_mut(mem.buf.as_mut_ptr() as *mut u32, mem.buf.len() / 4) };
    s[0] = 0;
    s[1] = cid as u32;
    s[2] = disc.map(|d| d[0]).unwrap_or_default();
    s[3] = disc.map(|d| d[1]).unwrap_or_default();
    s[4] = disc.map(|d| d[2]).unwrap_or_default();
    s[5] = disc.map(|d| d[3]).unwrap_or_default();
    mem.valid = None;
    mem.offset = None;
}

fn main() -> ! {
    use implementation::*;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let name_server =
        xous::create_server_with_address(b"xous-name-server").expect("Couldn't create xousnames-server");

    let d11ctimeout = D11cTimeout::new();

    // When a connection is requested but the server does not yet exist, it gets
    // placed into this pool.
    let mut waiting_connections: Vec<MessageEnvelope> = vec![];

    // this limits the number of available servers to be requested to 128...!
    //let mut name_table = FnvIndexMap::<XousServerName, xous::SID, 128>::new();
    let mut name_table = CheckedHashMap::new();

    info!("started");
    loop {
        let mut msg = xous::receive_message(name_server).unwrap();
        log::trace!("received message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::Register) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let registration = buffer.to_original::<Registration, _>().unwrap();
                let name = XousServerName::from_str(
                    registration.name.as_str().expect("couldn't convert server name to string"),
                );

                let response: api::Return;
                let mut should_connect = false;

                log::trace!("registration request for '{}'", name);
                if !name_table.contains_key(&name) {
                    let new_sid = xous::create_server_id().expect("create server failed, maybe OOM?");
                    name_table
                        .insert(name, new_sid, registration.conn_limit)
                        .expect("register name failure, maybe out of HashMap capacity?");
                    log::trace!("request successful, SID is {:?}", new_sid);
                    should_connect = true;
                    response = api::Return::SID(new_sid.into());
                } else {
                    info!("request failed, waiting for deterministic timeout");
                    d11ctimeout.deterministic_busy_wait();
                    info!("deterministic timeout done");
                    response = api::Return::Failure
                }
                buffer.replace(response).expect("Register can't serialize return value");

                // Drop the message, which causes it to get sent back to the sender.
                // The sender will then create the server immediately, allowing us
                // to connect any waiters to it.
                drop(buffer);
                drop(msg);

                if should_connect {
                    // See if we have any requests matching this server ID. If so, make the
                    // connection. Note that this could be replaced by `drain_filter()` when
                    // that is stabilized
                    let mut i = waiting_connections.len() as isize - 1;
                    while i >= 0 {
                        if name_from_msg(&waiting_connections[i as usize]) == Ok(name) {
                            let mut msg = waiting_connections.remove(i as usize);
                            match blocking_connect(&mut msg, &mut name_table) {
                                Err(e) => respond_connect_error(msg, e),
                                Ok(ConnectSuccess::Connected(cid, disc)) => {
                                    respond_connect_success(msg, cid, disc)
                                }
                                Ok(ConnectSuccess::Wait) => {
                                    panic!(
                                        "message connection attempt resulted in `Wait` even though it ought to exist"
                                    );
                                }
                            }
                        }
                        i -= 1;
                    }
                }
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
            Some(api::Opcode::BlockingConnect) | Some(api::Opcode::TryConnect) => {
                if !msg.body.is_blocking() {
                    continue;
                }
                if !msg.body.has_memory() {
                    xous::return_scalar(msg.sender, 0).unwrap();
                    continue;
                }

                match blocking_connect(&mut msg, &mut name_table) {
                    Err(e) => respond_connect_error(msg, e),
                    Ok(ConnectSuccess::Connected(cid, disc)) => respond_connect_success(msg, cid, disc),
                    Ok(ConnectSuccess::Wait) => {
                        if msg.body.id() == api::Opcode::TryConnect as usize {
                            respond_connect_error(msg, ConnectError::ServerNotFound);
                        } else {
                            // Push waiting connections here, which will prevent it from getting
                            // dropped and responded to.
                            waiting_connections.push(msg);
                        }
                    }
                }
            }
            Some(api::Opcode::Lookup) => {
                let mem = msg.body.memory_message_mut().unwrap();
                let mut buffer = unsafe { Buffer::from_memory_message_mut(mem) };
                let name_string = buffer.to_original::<String<64>, _>().unwrap();
                let name = XousServerName::from_str(
                    name_string.as_str().expect("couldn't convert server name to string"),
                );
                log::trace!("Lookup request for '{}'", name);
                let response: api::Return;
                if let (Some(server_sid), token) = name_table.connect(&name) {
                    let sender_pid = msg.sender.pid().expect("can't extract sender PID on Lookup");
                    match xous::connect_for_process(sender_pid, server_sid).expect("can't broker connection")
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
                    // no authenticate remedy currently supported, but we'd put that code somewhere around
                    // here eventually.
                    let (c1, c2, c3, c4) = xous::create_server_id().unwrap().to_u32();
                    let auth_request = AuthenticateRequest {
                        name: String::<64>::from_str(
                            name_string.as_str().expect("couldn't convert server name to string"),
                        ),
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
