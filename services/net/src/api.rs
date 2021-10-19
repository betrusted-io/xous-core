pub(crate) mod udp;
pub(crate) use udp::*;

use rkyv::{Archive, Deserialize, Serialize};
use std::net::{SocketAddr, IpAddr};
use smoltcp::wire::IpAddress;
use std::convert::TryInto;

pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

/// Dispatch opcodes to the Net crate main loop.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Calls for UDP implementation
    UdpBind,
    UdpClose,
    UdpTx,
    UdpSetTtl,
    UdpGetTtl,

    // The DNS server can hook the Net crate for notifications on config updates
    /// Adds an Ipv4 as a DNS server
    DnsHookAddIpv4,
    /// Adds an Ipv6 as a DNS server. Separate messages because max scalar arg is 128 bits.
    DnsHookAddIpv6,
    /// Called on IP config update -- clears all DNS servers.
    DnsHookAllClear,
    DnsUnhookAll,

    /// initiates a ping packet
    //PingSend,

    /// subscription for network responses
    //NetCallbackSubscribe,

    /// [Internal] com llio interrupt callback
    ComInterrupt,
    /// [Internal] run the network stack code
    NetPump,
    /// Suspend/resume callback
    SuspendResume,
    /// Quit the server
    Quit
}

/// These opcodes are reserved for private SIDs shared from a DNS server to
/// reconfigure DNS on IP change/update.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum PrivateDnsOp {
    AddIpv4DnsServer,
    AddIpv6DnsServer,
    RemoveIpv4DnsServer,
    RemoveIpv6DnsServer,
    RemoveAllServers,
    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCallback {
    Ping,
    Drop,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) enum NetMemResponse {
    Ok,
    Sent(u16),
    OutOfMemory,
    SocketInUse,
    AccessDenied,
    Invalid,
    LibraryError,
    AlreadyUsed,
}

/////// a bunch of structures are re-derived here so we can infer `rkyv` traits on them
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub (crate) struct NetSocketAddr {
    pub(crate) addr: NetIpAddr,
    pub(crate) port: u16,
}
impl From<SocketAddr> for NetSocketAddr {
    fn from(other: SocketAddr) -> NetSocketAddr {
        NetSocketAddr {
            addr: NetIpAddr::from(other),
            port: other.port(),
        }
    }
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub enum NetIpAddr {
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
}
impl From<SocketAddr> for NetIpAddr {
    fn from(other: SocketAddr) -> NetIpAddr {
        match other {
            SocketAddr::V4(sav4) => {
                NetIpAddr::Ipv4(sav4.ip().octets())
            },
            SocketAddr::V6(sav6) => {
                NetIpAddr::Ipv6(sav6.ip().octets())
            }
        }
    }
}
impl From<IpAddress> for NetIpAddr {
    fn from(other: IpAddress) -> NetIpAddr {
        match other {
            IpAddress::Ipv4(ipv4) => {
                NetIpAddr::Ipv4(ipv4.0)
            },
            IpAddress::Ipv6(ipv6) => {
                NetIpAddr::Ipv6(ipv6.0)
            },
            _ => {
                panic!("Invalid IpAddress")
            }
        }
    }
}
impl From<IpAddr> for NetIpAddr {
    fn from(other: IpAddr) -> NetIpAddr {
        match other {
            IpAddr::V4(ipv4) => {
                NetIpAddr::Ipv4(ipv4.octets())
            },
            IpAddr::V6(ipv6) => {
                NetIpAddr::Ipv6(ipv6.octets())
            }
        }
    }
}
impl From<NetIpAddr> for IpAddress {
    fn from(other: NetIpAddr) -> IpAddress {
        match other {
            NetIpAddr::Ipv4([a, b, c, d]) => {
                IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(a, b, c, d))
            }
            NetIpAddr::Ipv6(ipv6) => {
                    IpAddress::Ipv6(smoltcp::wire::Ipv6Address::new(
                        u16::from_be_bytes(ipv6[0..1].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[2..3].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[4..5].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[6..7].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[8..9].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[10..11].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[12..13].try_into().unwrap()),
                        u16::from_be_bytes(ipv6[14..15].try_into().unwrap()),
                    )
                )
            }
        }
    }
}
/* // can't quite figure this one out. oh well.
impl TryFrom<dyn ToSocketAddrs> for IpAddress {
    fn try_from(socket: dyn ToSocketAddrs) -> Result<IpAddress, xous::Error> {
        match socket.to_socket_addrs() {
            Ok(socks) => {
                match socks.into_iter().next() {
                    Some(socket_addr) => {
                        Ok(
                            match socket_addr {
                                std::net::SocketAddr::V4(v4addr) => {
                                    IpAddress::Ipv4(v4addr.ip())
                                }
                                std::net::SocketAddr::V6(v6addr) => {
                                    IpAddress::Ipv6(v6addr.ip())
                                }
                            }
                        )
                    }
                    _ => Err(xous::Error::InvalidString)
                }
            }
            _ => Err(xous::Error::InvalidString)
        }
    }
}
*/

/// This defines a Xous Scalar message endpoint. Used for defining
/// notification messages for incoming packets, for main loops that want to be
/// able to process both events from Xous and from the network.
pub(crate) struct XousScalarEndpoint {
    cid: Option<xous::CID>,
    op: Option<usize>,
    args: [Option<usize>; 4],
}
#[allow(dead_code)]
impl XousScalarEndpoint {
    pub(crate) fn new() -> Self {
        XousScalarEndpoint {
            cid: None,
            op: None,
            args: [None; 4]
        }
    }
    pub(crate) fn set(&mut self, cid: xous::CID, op: usize, args: [Option<usize>; 4]) {
        self.cid = Some(cid);
        self.op = Some(op);
        self.args = args;
    }
    pub(crate) fn clear(&mut self) {
        self.cid = None;
        self.op = None;
        self.args = [None; 4];
    }
    pub(crate) fn is_set(&self) -> bool {
        self.cid.is_some() && self.op.is_some()
    }
    pub(crate) fn notify(&mut self) {
        if let Some(cid) = self.cid {
            if let Some(op) = self.op {
                match xous::send_message(
                    cid,
                    xous::Message::new_scalar(
                        op,
                        if let Some(a) = self.args[0] {a} else {0},
                        if let Some(a) = self.args[1] {a} else {0},
                        if let Some(a) = self.args[2] {a} else {0},
                        if let Some(a) = self.args[3] {a} else {0},
                    )
                ) {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("Couldn't send scalar notification, unmapping: {:?}", e);
                        self.clear();
                    }
                }
            }
        }
    }
    /// We use u32 as the custom args instead of usize because we need
    /// the code to be portable to both 32 bit and 64 bit architectures. Code
    /// that assumes a 64-bit usize for the args on a 64-bit arch won't run on
    /// a 32-bit machine, so limit the max arg size to 32 bits.
    pub(crate) fn notify_custom_args(&mut self, custom: [Option<u32>; 4]) {
        if let Some(cid) = self.cid {
            if let Some(op) = self.op {
                match xous::send_message(
                    cid,
                    xous::Message::new_scalar(
                        op,
                        if let Some(b) = custom[0] {b as usize} else { if let Some(a) = self.args[0] {a} else {0} },
                        if let Some(b) = custom[1] {b as usize} else { if let Some(a) = self.args[1] {a} else {0} },
                        if let Some(b) = custom[2] {b as usize} else { if let Some(a) = self.args[2] {a} else {0} },
                        if let Some(b) = custom[3] {b as usize} else { if let Some(a) = self.args[3] {a} else {0} },
                    )
                ) {
                    Ok(_) => (),
                    Err(e) => {
                        log::error!("Couldn't send scalar notification, unmapping: {:?}", e);
                        self.clear();
                    }
                }
            }
        }
    }
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub struct XousPrivateServerScalarHook {
    /// The SID shared here should be dedicated only to responding to this hook
    pub one_time_sid: [u32; 4],
    /// Opcode discriminant of the response message
    pub op: usize,
    /// Any args you want in the scalar; depends on the application
    pub args: [Option<usize>; 4],
}
