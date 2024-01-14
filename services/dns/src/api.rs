#[allow(dead_code)]
// note: this name cannot be changed, because it is baked into `libstd`
pub(crate) const SERVER_NAME_DNS: &str = "_DNS Resolver Middleware_";
use net::NetIpAddr;
use rkyv::{Archive, Deserialize, Serialize};

#[allow(dead_code)]
pub(crate) const DNS_NAME_LENGTH_LIMIT: usize = 256;
#[allow(dead_code)]
pub(crate) const DNS_PKT_MAX_LEN: usize = 512;

/// These opcodes can be called by anyone at any time
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
#[repr(C)]
pub(crate) enum Opcode {
    Lookup = 0,
    Flush = 1,

    /// used internally to update the TTL field, and eventually expire the cache (unless cache is frozen)
    UpdateTtl = 2,

    /// issuing this opcode causes all future attempts to change the DNS server configs to be ignored. This
    /// also freezes the cache.
    FreezeConfig = 3,

    /// this allows automatic updates to the DNS server configs based on DHCP. This is the default state.
    ThawConfig = 4,

    Quit = 5,

    /// Perform a DNS lookup and return the results in a raw format.
    ///
    /// The query should be a `MutableBorrow` and should be a `&str` with the
    /// `valid` parameter set to the length of the query.
    ///
    /// The result will be a `&[u8]` with the first field being `0` and the second
    /// field indicating the number of results.
    ///
    /// # Errors
    ///
    /// If there is an error, then the first field is `1` and the second field is
    /// an error code corresponding to `DnsResponseCode`
    ///
    /// # Success
    ///
    /// Upon successful return, a series of entries will begin at offset 2. Each
    /// entry is a tag, followed by a number of octets. The tag indicates
    /// how many octets follow:
    ///
    ///     * 4: Ipv4 Address -- 4 octets follow, for a total of 5 bytes
    ///     * 6: Ipv6 Address -- 16 octets follow, for a total of 17 bytes
    RawLookup = 6,
}

#[derive(
    Debug, num_derive::FromPrimitive, num_derive::ToPrimitive, Archive, Serialize, Deserialize, Copy, Clone,
)]
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

// Time API items. Time is in the DNS crate because it has the resources
// to accommodate the time server, while the more logically grouped status
// crate does not.
pub const TIME_UX_NAME: &'static str = "_time UX server_";
/// Time API exports
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum TimeUxOp {
    SetTime = 0,
    SetTimeZone = 1,
    Quit = 2,
}
