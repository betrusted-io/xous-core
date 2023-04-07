#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
mod time; // why is this here? because it's the only place it'll fit. :-/
use api::*;

use net::NetIpAddr;
use num_traits::*;
use xous::msg_scalar_unpack;

use std::collections::HashMap;
use std::convert::TryInto;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::Duration;
use std::thread;
use xous_ipc::{Buffer, String};

// KISS DNS

// The DNS implementation here is based on https://github.com/vinc/moros/blob/43ac7cdc8ccc860dc1b6f0f060b5dbcd01424c03/src/usr/host.rs
// MOROS is MIT licensed.
// See RFC 1035 for implementation details

#[repr(u16)]
enum QueryType {
    A = 1,
    // NS = 2,
    // MD = 3,
    // MF = 4,
    // CNAME = 5,
    // SOA = 6,
    // MX = 15,
    // TXT = 16,
}

#[repr(u16)]
enum QueryClass {
    IN = 1,
}

struct Message {
    pub datagram: Vec<u8>,
}

const FLAG_RD: u16 = 0x0100; // Recursion desired

impl Message {
    pub fn from(datagram: &[u8]) -> Self {
        Self {
            datagram: Vec::from(datagram),
        }
    }

    pub fn query(qname: &str, qtype: QueryType, qclass: QueryClass, id: u16) -> Self {
        let mut datagram = Vec::new();

        for b in id.to_be_bytes().iter() {
            datagram.push(*b); // Transaction ID
        }
        for b in FLAG_RD.to_be_bytes().iter() {
            datagram.push(*b); // Flags
        }
        for b in (1 as u16).to_be_bytes().iter() {
            datagram.push(*b); // Questions
        }
        for _ in 0..6 {
            datagram.push(0); // Answer + Authority + Additional
        }
        for label in qname.split('.') {
            datagram.push(label.len() as u8); // QNAME label length
            for b in label.bytes() {
                datagram.push(b); // QNAME label bytes
            }
        }
        datagram.push(0); // Root null label
        for b in (qtype as u16).to_be_bytes().iter() {
            datagram.push(*b); // QTYPE
        }
        for b in (qclass as u16).to_be_bytes().iter() {
            datagram.push(*b); // QCLASS
        }

        Self { datagram }
    }

    pub fn id(&self) -> u16 {
        u16::from_be_bytes(self.datagram[0..2].try_into().unwrap())
    }

    pub fn header(&self) -> u16 {
        u16::from_be_bytes(self.datagram[2..4].try_into().unwrap())
    }

    pub fn is_response(&self) -> bool {
        if (self.header() & (1 << 15)) == 0 {
            false
        } else {
            true
        }
    }

    fn fast_foward_name(&self, start: usize) -> Result<usize, DnsResponseCode> {
        use DnsResponseCode::FormatError;
        let mut index = start;
        loop {
            log::trace!("cname index: {}", index);
            if *(self.datagram.get(index).ok_or(FormatError)?) == 0 {
                index += 1;
                break;
            } else {
                index += *(self.datagram.get(index).ok_or(FormatError)?) as usize;
                index += 1;
            }
        }
        Ok(index)
    }

    pub fn parse_response(&self) -> Result<HashMap<IpAddr, u32>, DnsResponseCode> {
        use DnsResponseCode::FormatError;
        log::trace!("parsing packet: {:?}", self.datagram);

        let mut map = HashMap::<IpAddr, u32>::new();
        // ASSUME: the query ID and response bit fields have already been checked
        // and that the rcode is valid
        let qdcount = u16::from_be_bytes(self.datagram[4..6].try_into().unwrap());
        let ancount = u16::from_be_bytes(self.datagram[6..8].try_into().unwrap());

        let mut index = 12;
        // fast forward past the qname
        for queries in 0..qdcount {
            log::trace!("parsing query{}, index {}", queries, index);
            index = self.fast_foward_name(index)?;
            log::trace!("fast forward through qname to {}", index);
            // index is now at qtype
            let qtype = u16::from_be_bytes(self.datagram[index..index + 2].try_into().unwrap());
            // A = 1, AAAA = 28
            if qtype != 1 && qtype != 28 {
                log::error!("Problem parsing qname, qtype is not 1 or 28: {}", qtype);
                return Err(FormatError);
            }
            index += 2;
            let qclass = u16::from_be_bytes(self.datagram[index..index + 2].try_into().unwrap());
            if qclass != 1 {
                log::error!("Problem parsing qname, qclass is not 1: {}", qclass);
                return Err(FormatError);
            }
            index += 2;
        }
        // index is now at the aname section
        for aname in 0..ancount {
            log::trace!("parsing aname{}, index {}", aname, index);
            // first check to see if we're dealing with a pointer or a name
            if self.datagram[index] >= 0xc0 {
                // pointer
                index += 1;
                if self.datagram[index] != 0xc {
                    log::error!(
                        "Found aname pointer, but value does not conform to length of aname header"
                    );
                    return Err(FormatError);
                }
                index += 1;
            } else {
                // name, fast forward past the name
                index = self.fast_foward_name(index)?;
                log::trace!("fast forward aname to {}", index);
            }
            // index is now at type
            let atype = u16::from_be_bytes(self.datagram[index..index + 2].try_into().unwrap());
            // A = 1, AAAA = 28
            if atype != 1 && atype != 28 {
                log::error!("Problem parsing aname, type is not 1 or 28: {}", atype);
                return Err(FormatError);
            }
            index += 2;
            let aclass = u16::from_be_bytes(self.datagram[index..index + 2].try_into().unwrap());
            if aclass != 1 {
                log::error!("Problem parsing aname, aclass is not 1: {}", aclass);
                return Err(FormatError);
            }
            index += 2;
            // this is our TTL
            let ttl = u32::from_be_bytes(self.datagram[index..index + 4].try_into().unwrap());
            log::trace!("got ttl: {}", ttl);
            index += 4;
            // this is the payload length
            let addr_len = u16::from_be_bytes(self.datagram[index..index + 2].try_into().unwrap());
            index += 2;
            match addr_len {
                // ipv4
                4 => {
                    if atype != 1 {
                        log::error!("Got a 4-byte address, but ATYPE != A (1)");
                        return Err(FormatError);
                    }
                    // this copy happens because I can't figure out how to get Ipv4Addr::from() to realize it's casting from a [u8;4]
                    let mut rdata: [u8; 4] = [0; 4];
                    for (&src, dst) in self.datagram[index..index + 4].iter().zip(rdata.iter_mut())
                    {
                        *dst = src;
                    }
                    let addr = IpAddr::V4(Ipv4Addr::from(rdata));
                    index += 4;
                    map.insert(addr, ttl);
                }
                // ipv6
                16 => {
                    if atype != 28 {
                        log::error!("Got a 16-byte address, but ATYPE != AAAA (28)");
                        return Err(FormatError);
                    }
                    // this copy happens because I can't figure out how to get Ipv6Addr::from() to realize it's casting from a [u8;4]
                    let mut rdata: [u8; 16] = [0; 16];
                    for (&src, dst) in self.datagram[index..index + 16]
                        .iter()
                        .zip(rdata.iter_mut())
                    {
                        *dst = src;
                    }
                    let addr = IpAddr::V6(Ipv6Addr::from(rdata));
                    index += 16;
                    map.insert(addr, ttl);
                }
                _ => {
                    log::error!("Length field does not match a known record type");
                    return Err(FormatError);
                }
            }
        }

        Ok(map)
    }

    /*
         example response for: betrusted.io->185.199.111.153
    Header:
          61, ca,   id
          81, 80,   header
          0, 1,     qdcount
          0, 4,     ancount
          0, 0,     nscount
          0, 0,     arcount
    qname:
          9,        length 9
          62, 65, 74, 72, 75, 73, 74, 65, 64,    "betrusted"
          2,        length 2
          69, 6f,   "io"
          0,        end of name
    qtype:
          0, 1,     type A
    qclass:
          0, 1,     type IN
    aname0:
          c0,       name is a pointer (any value > 192 is a pointer)
          c,        offset of 12 from start of aname0
          0, 1,     type A
          0, 1,     class IN
          0, 0, e, 10,   0xe10 = 3600 seconds TTL
          0, 4,     4 bytes address
          b9, c7, 6c, 99,  address
    aname1:
          c0,       name is a pointer
          c,
          0, 1,     type A
          0, 1,     class IN
          0, 0, e, 10,  TTL
          0, 4,     4 byte address
          b9, c7, 6d, 99,  address
    aname2:
          c0,
          c,
          0, 1,
          0, 1,
          0, 0, e, 10,
          0, 4,
          b9, c7, 6e, 99,
    aname3:
          c0,
          c,
          0, 1,
          0, 1,
          0, 0, e, 10,
          0, 4,
          b9, c7, 6f, 99
         */

    /*
    pub fn is_query(&self) -> bool {
        !self.is_response()
    }
    */

    pub fn rcode(&self) -> DnsResponseCode {
        match (self.header() >> 11) & 0xF {
            0 => DnsResponseCode::NoError,
            1 => DnsResponseCode::FormatError,
            2 => DnsResponseCode::ServerFailure,
            3 => DnsResponseCode::NameError,
            4 => DnsResponseCode::NotImplemented,
            5 => DnsResponseCode::Refused,
            _ => DnsResponseCode::UnknownError,
        }
    }
}

pub struct Resolver {
    /// DnsServerManager is a service of the Net crate that automatically updates the DNS server list
    mgr: net::DnsServerManager,
    socket: UdpSocket,
    buf: [u8; DNS_PKT_MAX_LEN],
    trng: trng::Trng,
    freeze: bool,
}
impl Resolver {
    pub fn new(xns: &xous_names::XousNames) -> Resolver {
        let trng = trng::Trng::new(&xns).unwrap();
        let local_port = (49152 + trng.get_u32().unwrap() % 16384) as u16;
        let socket = UdpSocket::bind(
            format!("0.0.0.0:{}", local_port),
        )
        .expect("couldn't create socket for DNS resolver");
        let timeout = Duration::from_millis(10_000); // 10 seconds for DNS to resolve by default
        socket.set_read_timeout(Some(timeout)).unwrap();
        socket.set_nonblocking(false).unwrap(); // we want this to block.
                                                // we /could/ do a non-blocking DNS resolver, but...what would you do in the meantime??
                                                // blocking is probably what we actually want this time.

        Resolver {
            mgr: net::DnsServerManager::register(&xns)
                .expect("Couldn't register the DNS server list auto-manager"),
            socket,
            buf: [0; DNS_PKT_MAX_LEN],
            trng,
            freeze: false,
        }
    }
    pub fn add_server(&mut self, addr: IpAddr) {
        self.mgr.add_server(addr);
    }
    pub fn remove_server(&mut self, addr: IpAddr) {
        self.mgr.remove_server(addr);
    }
    pub fn clear_all_servers(&mut self) {
        self.mgr.clear();
    }
    pub fn set_freeze_config(&mut self, freeze: bool) {
        self.freeze = freeze;
        self.mgr.set_freeze(freeze);
    }
    pub fn get_freeze(&self) -> bool {
        self.freeze
    }
    /// this allows us to re-use the TRNG object
    pub fn trng_u32(&self) -> u32 {
        self.trng.get_u32().unwrap()
    }
    pub fn resolve(&mut self, name: &str) -> Result<HashMap<IpAddr, u32>, DnsResponseCode> {
        if let Some(dns_address) = self.mgr.get_random() {
            let dns_port = 53;
            let server = SocketAddr::new(dns_address, dns_port);

            let qname = name;
            let qtype = QueryType::A;
            let qclass = QueryClass::IN;
            let query = Message::query(qname, qtype, qclass, self.trng.get_u32().unwrap() as u16);

            self.socket
                .send_to(&query.datagram, &server)
                .map_err(|_| DnsResponseCode::NetworkError)?;

            match self.socket.recv(&mut self.buf) {
                Ok(len) => {
                    let message = Message::from(&self.buf[..len]);
                    if message.id() == query.id() && message.is_response() {
                        return match message.rcode() {
                            DnsResponseCode::NoError => message.parse_response(),
                            rcode => Err(rcode),
                        };
                    } else {
                        Err(DnsResponseCode::NetworkError)
                    }
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => Err(DnsResponseCode::NetworkError),
                    _ => Err(DnsResponseCode::UnknownError),
                },
            }
        } else {
            Err(DnsResponseCode::NoServerSpecified)
        }
    }
}

#[derive(PartialEq, Debug)]
#[repr(C)]
enum NameConversionError {
    /// The length of the memory buffer was invalid
    InvalidMemoryBuffer = 1,

    /// The specified nameserver string was not UTF-8
    InvalidString = 3,

    /// The message was not a mutable memory message
    InvalidMessageType = 4,
}

fn name_from_msg(env: &xous::MessageEnvelope) -> Result<&str, NameConversionError> {
    let msg = env
        .body
        .memory_message()
        .ok_or(NameConversionError::InvalidMessageType)?;
    let valid_bytes = msg.valid.map(|v| v.get()).unwrap_or_else(|| msg.buf.len());
    if valid_bytes > DNS_NAME_LENGTH_LIMIT || valid_bytes < 1 {
        log::error!("valid bytes exceeded DNS name limit");
        return Err(NameConversionError::InvalidMemoryBuffer);
    }
    // Safe because we've already validated that it's a valid range
    let str_slice = unsafe { core::slice::from_raw_parts(msg.buf.as_ptr(), valid_bytes) };
    let name_string =
        core::str::from_utf8(str_slice).map_err(|_| NameConversionError::InvalidString)?;

    Ok(name_string)
}

fn fill_response(mut env: xous::MessageEnvelope, entries: &HashMap<IpAddr, u32>) -> Option<()> {
    let mem = env.body.memory_message_mut()?;

    let s: &mut [u8] = mem.buf.as_slice_mut();
    let mut i = s.iter_mut();

    // First tag = 1 for "Error" -- we'll fill this in at the end when it's successful
    *i.next()? = 1;

    // Limit the number of entries to 128, which is a nice number. Given that an IPv6
    // address is 17 bytes, that means that ~240 IPv6 addresses will fit in a 4 kB page.
    // 128 is just a conservative value rounded down.
    let mut entry_count = entries.len();
    if entry_count > 128 {
        entry_count = 128;
    }
    *i.next()? = entry_count.try_into().ok()?;

    // Start filling in the addreses
    for addr in entries.keys() {
        match addr {
            &IpAddr::V4(a) => {
                // IPv4
                *i.next()? = 4;
                for entry in a.octets() {
                    *i.next()? = entry;
                }
            }
            &IpAddr::V6(a) => {
                // IPv6
                for entry in a.octets() {
                    *i.next()? = entry;
                }
                *i.next()? = 6;
            }
        }
    }

    // Convert the entry to a "Success" message
    drop(i);
    s[0] = 0;

    None
}

fn fill_error(mut env: xous::MessageEnvelope, code: DnsResponseCode) -> Option<()> {
    let mem = env.body.memory_message_mut()?;

    let s: &mut [u8] = mem.buf.as_slice_mut();
    let mut i = s.iter_mut();

    *i.next()? = 1;
    *i.next()? = code as u8;
    None
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // Time is stuck in the DNS crate because the status crate is out of resources, and the DNS
    // crate is fairly under-utilized and ideal for sticking a service like time in it.
    //
    // this kicks off the thread that services the `libstd` calls for time-related things.
    // we want this started really early, because it sanity checks the RTC and a bunch of other stuff.
    time::start_time_server();
    time::start_time_ux();

    let xns = xous_names::XousNames::new().unwrap();
    let dns_sid = xns
        .register_name(api::SERVER_NAME_DNS, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", dns_sid);

    // this will magically populate a list of DNS servers when they become available
    let mut resolver = Resolver::new(&xns);
    // if you wanted to force a server into the initial config, you can do it here, for example:
    // resolver.add_server(IpAddr::V4(Ipv4Addr::new(1,1,1,1)));

    // the `u32` value is the TTL of the IpAddr
    let mut dns_cache = HashMap::<std::string::String, HashMap<IpAddr, u32>>::new();

    // build a thread that pings the UpdateTtl function once every few minutes to expire the DNS cache
    thread::spawn({
        let local_cid = xous::connect(dns_sid).unwrap();
        move || {
            const TTL_INTERVAL_SECS: usize = 300; // every 5 minutes update the map
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            loop {
                tt.sleep_ms(TTL_INTERVAL_SECS * 1000).unwrap();
                xous::send_message(
                    local_cid,
                    xous::Message::new_scalar(
                        Opcode::UpdateTtl.to_usize().unwrap(),
                        TTL_INTERVAL_SECS,
                        0,
                        0,
                        0,
                    ),
                )
                .expect("couldn't increment DNS cache");
            }
        }
    });

    log::trace!("ready to accept requests");
    loop {
        let mut msg = xous::receive_message(dns_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::RawLookup) => {
                match name_from_msg(&msg).map(|s| s.to_owned()) {
                    Ok(owned_name) => {
                        // handle the special case of "localhost" as a string
                        if owned_name == "localhost" {
                            let mut local = HashMap::<IpAddr, u32>::new();
                            local.insert(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 86400);
                            fill_response(msg, &local);
                            continue;
                        }
                        log::trace!("performing a lookup of {}", owned_name);
                        // Try to get the result out of the DNS cache
                        if let Some(entries) = dns_cache.get(&owned_name) {
                            fill_response(msg, entries);
                            continue;
                        }

                        // This entry is not in the cache, so perform a lookup
                        match resolver.resolve(&owned_name) {
                            Ok(cache_entry) => {
                                fill_response(msg, &cache_entry);
                                dns_cache.insert(owned_name, cache_entry);
                                continue;
                            }
                            Err(e) => {
                                fill_error(msg, e);
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("unable to do name lookup: {:?}", e);
                        fill_error(msg, DnsResponseCode::NameError);
                        continue;
                    }
                };
            }
            Some(Opcode::Lookup) => {
                let mut buf = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let name = buf
                    .to_original::<String<DNS_NAME_LENGTH_LIMIT>, _>()
                    .unwrap();
                let name_std = std::string::String::from(name.as_str().unwrap());
                if let Some(cache_entry) = dns_cache.get(&name_std) {
                    // pick a random entry
                    let rand = resolver.trng_u32() as usize % cache_entry.len();
                    for (index, (ip_addr, _)) in cache_entry.iter().enumerate() {
                        if rand == index {
                            log::debug!("DNS cached: {}->{:?}", name, ip_addr);
                            let response = DnsResponse {
                                addr: Some(NetIpAddr::from(*ip_addr)),
                                code: DnsResponseCode::NoError,
                            };
                            buf.replace(response).unwrap();
                            break;
                        }
                    }
                } else {
                    match resolver.resolve(name.as_str().unwrap()) {
                        Ok(cache_entry) => {
                            if cache_entry.len() > 0 {
                                dns_cache.insert(name_std, cache_entry);

                                // now pick the entry back out again, as it was consumed...
                                let name_std = std::string::String::from(name.as_str().unwrap());
                                let cache_entry = dns_cache.get(&name_std).unwrap();

                                // pick a random entry from the query response
                                let rand = resolver.trng_u32() as usize % cache_entry.len();
                                for (index, (ip_addr, _)) in cache_entry.iter().enumerate() {
                                    if rand == index {
                                        let response = DnsResponse {
                                            addr: Some(NetIpAddr::from(*ip_addr)),
                                            code: DnsResponseCode::NoError,
                                        };
                                        buf.replace(response).unwrap();
                                        break;
                                    }
                                }
                            } else {
                                // no names found
                                let response = DnsResponse {
                                    addr: None,
                                    code: DnsResponseCode::NameError,
                                };
                                buf.replace(response).unwrap();
                            }
                        }
                        Err(e) => {
                            log::debug!("DNS query failed: {}->{:?}", name, e);
                            let response = DnsResponse {
                                addr: None,
                                code: e,
                            };
                            buf.replace(response).unwrap();
                        }
                    }
                }
            }
            Some(Opcode::UpdateTtl) => msg_scalar_unpack!(msg, incr_secs, _, _, _, {
                let increment = if incr_secs < u32::MAX as usize {
                    incr_secs as u32
                } else {
                    u32::MAX
                };
                if !resolver.get_freeze() {
                    let mut expired_names = Vec::<std::string::String>::new();
                    for (name, cache_map) in dns_cache.iter_mut() {
                        // each entry can have multiple names with a different TTL
                        // decrement the TTL, and note which go to zero
                        let mut expired_entries = Vec::<IpAddr>::new();
                        for (entry, ttl) in cache_map.iter_mut() {
                            log::debug!("entry: {:?}, ttl: {}, incr: {}", entry, ttl, increment);
                            if *ttl < increment {
                                *ttl = 0;
                                expired_entries.push(*entry);
                            } else {
                                *ttl = *ttl - increment as u32;
                            }
                        }
                        // remove the entries that are 0
                        for entry in expired_entries {
                            log::debug!("DNS cache expiring {:?}", entry);
                            cache_map.remove(&entry);
                        }
                        // if all the entries are removed, mark for removal from the cache entirely
                        if cache_map.len() == 0 {
                            // have to copy the name to a new object to track it
                            let name = std::string::String::from(name.as_str());
                            expired_names.push(name);
                        }
                    }
                    for name in expired_names {
                        log::debug!("DNS cache removing {}", &name);
                        dns_cache.remove(&name);
                    }
                }
            }),
            Some(Opcode::Flush) => {
                dns_cache.clear();
            }
            Some(Opcode::FreezeConfig) => {
                resolver.set_freeze_config(true);
            }
            Some(Opcode::ThawConfig) => {
                resolver.set_freeze_config(false);
            }
            Some(Opcode::Quit) => {
                log::warn!("got quit!");
                break;
            }
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(dns_sid).unwrap();
    xous::destroy_server(dns_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
