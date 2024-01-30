// As of Rust 1.64.0:
//
// Rkyv-derived enums throw warnings that rkyv::Archive derived enums are never used
// and I can't figure out how to make them go away. Since they spam the build log,
// rkyv-derived enums are now isolated to their own file with a file-wide `dead_code`
// allow on top.
//
// This might be a temporary compiler regression, or it could just
// be yet another indicator that it's time to upgrade rkyv. However, we are waiting
// until rkyv hits 0.8 (the "shouldn't ever change again but still not sure enough
// for 1.0") release until we rework the entire system to chase the latest rkyv.
// As of now, the current version is 0.7.x and there isn't a timeline yet for 0.8.

#![allow(dead_code)]
use std::convert::TryInto;
use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use rkyv::{Archive, Deserialize, Serialize};
use smoltcp::wire::IpAddress;

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub enum XousServerId {
    /// A SID that is shared directly with the Net crate; a private, single-use SID for best security
    PrivateSid([u32; 4]),
    /// A name that needs to be looked up with XousNames. Easier to implement, but less secure as it requires
    /// an open connection slot that could be abused by other processes.
    ServerName(xous_ipc::String<64>),
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
#[repr(C, u16)]
pub(crate) enum NetMemResponse {
    Ok,
    Sent(u16),
    OutOfMemory,
    SocketInUse,
    AccessDenied,
    Invalid,
    Finished,
    LibraryError,
    AlreadyUsed,
}

#[derive(Archive, Serialize, Deserialize, Copy, Clone)]
pub enum NetIpAddr {
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
}
impl From<SocketAddr> for NetIpAddr {
    fn from(other: SocketAddr) -> NetIpAddr {
        match other {
            SocketAddr::V4(sav4) => NetIpAddr::Ipv4(sav4.ip().octets()),
            SocketAddr::V6(sav6) => NetIpAddr::Ipv6(sav6.ip().octets()),
        }
    }
}
impl From<IpAddress> for NetIpAddr {
    fn from(other: IpAddress) -> NetIpAddr {
        match other {
            IpAddress::Ipv4(ipv4) => NetIpAddr::Ipv4(ipv4.0),
            IpAddress::Ipv6(ipv6) => NetIpAddr::Ipv6(ipv6.0),
        }
    }
}
impl From<IpAddr> for NetIpAddr {
    fn from(other: IpAddr) -> NetIpAddr {
        match other {
            IpAddr::V4(ipv4) => NetIpAddr::Ipv4(ipv4.octets()),
            IpAddr::V6(ipv6) => NetIpAddr::Ipv6(ipv6.octets()),
        }
    }
}
impl From<NetIpAddr> for IpAddr {
    fn from(other: NetIpAddr) -> IpAddr {
        match other {
            NetIpAddr::Ipv4(octets) => IpAddr::V4(Ipv4Addr::from(octets)),
            NetIpAddr::Ipv6(octets) => IpAddr::V6(Ipv6Addr::from(octets)),
        }
    }
}
impl From<NetIpAddr> for IpAddress {
    fn from(other: NetIpAddr) -> IpAddress {
        match other {
            NetIpAddr::Ipv4([a, b, c, d]) => IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(a, b, c, d)),
            NetIpAddr::Ipv6(ipv6) => IpAddress::Ipv6(smoltcp::wire::Ipv6Address::new(
                u16::from_be_bytes(ipv6[0..1].try_into().unwrap()),
                u16::from_be_bytes(ipv6[2..3].try_into().unwrap()),
                u16::from_be_bytes(ipv6[4..5].try_into().unwrap()),
                u16::from_be_bytes(ipv6[6..7].try_into().unwrap()),
                u16::from_be_bytes(ipv6[8..9].try_into().unwrap()),
                u16::from_be_bytes(ipv6[10..11].try_into().unwrap()),
                u16::from_be_bytes(ipv6[12..13].try_into().unwrap()),
                u16::from_be_bytes(ipv6[14..15].try_into().unwrap()),
            )),
        }
    }
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TcpMgmtCode {
    SetRxShutdown,
    SetNoDelay(bool),
    GetNoDelay(bool),
    SetTtl(u32),
    GetTtl(u32),
    ErrorCheck(NetMemResponse),
    Flush(bool),
    CloseListener,
}
