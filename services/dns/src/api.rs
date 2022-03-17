#[allow(dead_code)]
pub(crate) const SERVER_NAME_DNS: &str = "_DNS Resolver Middleware_";
use net::NetIpAddr;
use rkyv::{Archive, Deserialize, Serialize};

#[allow(dead_code)]
pub(crate) const DNS_NAME_LENGTH_LIMIT: usize = 256;
#[allow(dead_code)]
pub(crate) const DNS_PKT_MAX_LEN: usize = 512;

/// These opcodes can be called by anyone at any time
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    Lookup,
    Flush,

    /// used internally to update the TTL field, and eventually expire the cache (unless cache is frozen)
    UpdateTtl,

    /// issuing this opcode causes all future attempts to change the DNS server configs to be ignored. This also freezes the cache.
    FreezeConfig,
    /// this allows automatic updates to the DNS server configs based on DHCP. This is the default state.
    ThawConfig,

    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, Archive, Serialize, Deserialize, Copy, Clone)]
#[repr(u16)]
pub enum DnsResponseCode {
    NoError = 0,
    FormatError = 1,
    ServerFailure = 2,
    NameError = 3,
    NotImplemented = 4,
    Refused = 5,

    UnknownError = 6,
    NetworkError = 7,
    NoServerSpecified = 8,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub struct DnsResponse {
    pub addr: Option<NetIpAddr>,
    pub code: DnsResponseCode,
}
