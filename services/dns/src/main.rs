#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use net::{NetIpAddr, Duration};
use num_traits::*;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::collections::HashMap;
use std::io::ErrorKind;
use xous_ipc::{String, Buffer};
use std::convert::TryInto;

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
    socket: net::UdpSocket,
    buf: [u8; DNS_PKT_MAX_LEN],
    trng: trng::Trng,
}
impl Resolver {
    pub fn new(xns: &xous_names::XousNames) -> Resolver {
        let trng = trng::Trng::new(&xns).unwrap();
        let local_port = (49152 + trng.get_u32().unwrap() % 16384) as u16;
        let mut socket = net::UdpSocket::bind_xous(
            format!("127.0.0.1:{}", local_port),
            Some(DNS_PKT_MAX_LEN as u16)
        ).expect("couldn't create socket for DNS resolver");
        let timeout = Duration::from_millis(10_000); // 10 seconds for DNS to resolve by default
        socket.set_read_timeout(Some(timeout)).unwrap();
        socket.set_nonblocking(false).unwrap(); // we want this to block.
        // we /could/ do a non-blocking DNS resolver, but...what would you do in the meantime??
        // blocking is probably what we actually want this time.

        Resolver {
            mgr: net::DnsServerManager::register(&xns).expect("Couldn't register the DNS server list auto-manager"),
            socket,
            buf: [0; DNS_PKT_MAX_LEN],
            trng,
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
        self.mgr.set_freeze(freeze);
    }
    pub fn resolve(&mut self, name: &str) -> Result<IpAddr, DnsResponseCode> {
        if let Some(dns_address) = self.mgr.get_random() {
            let dns_port = 53;
            let server = SocketAddr::new(dns_address, dns_port);

            let qname = name;
            let qtype = QueryType::A;
            let qclass = QueryClass::IN;
            let query = Message::query(qname, qtype, qclass, self.trng.get_u32().unwrap() as u16);

            self.socket.send_to(&query.datagram, &server)
            .map_err(|_| DnsResponseCode::NetworkError)?;

            match self.socket.recv(&mut self.buf) {
                Ok(len) => {
                    log::info!("buf {}: {:x?}", len, &self.buf[..len]);
                    let message = Message::from(&self.buf[..len]);
                    if message.id() == query.id() && message.is_response() {
                        return match message.rcode() {
                            DnsResponseCode::NoError => {
                                // TODO: Parse the datagram instead of
                                // extracting the last 4 bytes.
                                //let rdata = message.answer().rdata();
                                let n = message.datagram.len();
                                let mut rdata: [u8; 4] = [0; 4];
                                log::info!("datagram{}: {:x?}", n, message.datagram);
                                for (&src, dst) in message.datagram[(n - 4)..].iter().zip(rdata.iter_mut()) {
                                    *dst = src;
                                }
                                log::info!("rdata: {:?}", rdata);
                                Ok(IpAddr::V4(Ipv4Addr::from(rdata)))
                            }
                            rcode => {
                                Err(rcode)
                            }
                        }
                    } else {
                        Err(DnsResponseCode::NetworkError)
                    }
                }
                Err(e) => {
                    match e.kind() {
                        ErrorKind::WouldBlock => {
                            Err(DnsResponseCode::NetworkError)
                        }
                        _ => {
                            Err(DnsResponseCode::UnknownError)
                        }
                    }
                }
            }

        } else {
            Err(DnsResponseCode::NoServerSpecified)
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let dns_sid = xns.register_name(api::SERVER_NAME_DNS, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", dns_sid);

    // this will magically populate a list of DNS servers when they become available
    let mut resolver = Resolver::new(&xns);
    // if you wanted to force a server into the initial config, you can do it here, for example:
    // resolver.add_server(IpAddr::V4(Ipv4Addr::new(1,1,1,1)));

    // perhaps eventually this should be expanded to include a TTL and periodic sweep is done to clear the cache.
    // but for now, let's go for something simple.
    let mut dns_cache = HashMap::<std::string::String, IpAddr>::new();

    log::trace!("ready to accept requests");
    loop {
        let mut msg = xous::receive_message(dns_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Lookup) => {
                let mut buf = unsafe{Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())};
                let name = buf.to_original::<String::<DNS_NAME_LENGTH_LIMIT>, _>().unwrap();
                let name_std = std::string::String::from(name.as_str().unwrap());
                if let Some(ip_addr) = dns_cache.get(&name_std) {
                    log::info!("DNS cached: {}->{:?}", name, ip_addr);
                    let response = DnsResponse {
                        addr: Some(NetIpAddr::from(*ip_addr)),
                        code: DnsResponseCode::NoError,
                    };
                    buf.replace(response).unwrap();
                } else {
                    match resolver.resolve(name.as_str().unwrap()) {
                        Ok(ip_addr) => {
                            log::info!("DNS queried: {}->{:?}", name, ip_addr);
                            dns_cache.insert(name_std, ip_addr);
                            let response = DnsResponse {
                                addr: Some(NetIpAddr::from(ip_addr)),
                                code: DnsResponseCode::NoError,
                            };
                            buf.replace(response).unwrap();
                        },
                        Err(e) => {
                            log::info!("DNS query failed: {}->{:?}", name, e);
                            let response = DnsResponse {
                                addr: None,
                                code: e,
                            };
                            buf.replace(response).unwrap();
                        },
                    }
                }
            },
            Some(Opcode::Flush) => {
                dns_cache.clear();
            },
            Some(Opcode::FreezeConfig) => {
                resolver.set_freeze_config(true);
            },
            Some(Opcode::ThawConfig) => {
                resolver.set_freeze_config(false);
            }
            Some(Opcode::Quit) => {
                log::warn!("got quit!");
                break
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
