pub(crate) mod udp;
// needed to keep hosted mode quiet, since the Udp implementation is a bodge
#[allow(unused_imports)]
pub(crate) use udp::*;
pub(crate) mod ping;
pub(crate) use ping::*;
pub(crate) mod tcp;
pub use ping::NetPingCallback;
// needed to keep hosted mode quiet, since the Tcp implementation is a bodge
#[allow(unused_imports)]
pub(crate) use tcp::*;

use com::SsidRecord;
use rkyv::{Archive, Deserialize, Serialize};
use smoltcp::wire::IpAddress;
use std::convert::TryInto;
use std::fmt;
use std::fmt::Debug;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

// republish this so we can decode the icmpv4 error codes
pub use smoltcp::wire::Icmpv4DstUnreachable;

pub(crate) const SERVER_NAME_NET: &str = "_Middleware Network Server_";
#[allow(dead_code)]
pub const AP_DICT_NAME: &'static str = "wlan.networks";

#[allow(dead_code)]
/// minimum revision required for compatibility with Net crate
pub const MIN_EC_REV: u32 = 0x00_09_06_00;

/// Dispatch opcodes to the Net crate main loop.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
#[repr(C)]
pub(crate) enum Opcode {
    /// Calls for UDP implementation
    UdpBind = 0,
    UdpClose = 1,
    UdpTx = 2,
    UdpSetTtl = 3,
    UdpGetTtl = 4,

    /// Calls for TCP implementation
    TcpConnect = 5,
    TcpTx = 6,
    TcpClose = 7,
    TcpManage = 8,
    TcpListen = 9,
    TcpManageListener = 10,

    // The DNS server can hook the Net crate for notifications on config updates
    /// Adds an Ipv4 as a DNS server
    DnsHookAddIpv4 = 11,
    /// Adds an Ipv6 as a DNS server. Separate messages because max scalar arg is 128 bits.
    DnsHookAddIpv6 = 12,
    /// Called on IP config update -- clears all DNS servers.
    DnsHookAllClear = 13,
    DnsUnhookAll = 14,

    /// Ping stack
    Ping = 15,
    PingSetTtl = 16,
    PingGetTtl = 17,
    PingSetTimeout = 18,
    PingGetTimeout = 19,

    /// Link Management,
    GetIpv4Config = 20,
    Reset = 21,
    SubscribeWifiStats = 22,
    UnsubWifiStats = 23,
    FetchSsidList = 24,
    ConnMgrStartStop = 25,

    /// [Internal] com llio interrupt callback
    ComInterrupt = 26,
    /// [Internal] run the network stack code
    NetPump = 27,
    /// Suspend/resume callback
    SuspendResume = 28,
    /// Quit the server
    Quit = 29,

    /// Create a connection to the target address
    ///
    /// # Arguments
    ///
    ///  u8 array
    /// -------|---------
    /// offset | Contents
    /// =======|=========
    ///      0 | port number, low byte
    ///      1 | port number, high byte
    ///      2 | timeout, msecs, low byte
    ///      3 | timeout
    ///      4 | timeout
    ///      5 | timeout
    ///      6 | timeout
    ///      7 | timeout
    ///      8 | timeout
    ///      9 | timeout, high byte
    ///     10 | address type -- 4 = ipv4, 6 = ipv6
    ///    ... | remaining bytes are the address
    ///
    /// # Returns
    ///
    /// u16 array
    /// -------|---------
    /// offset | Contents
    /// =======|=========
    ///      0 | 0 indicating success
    ///      1 | Connection index
    ///      2 | local port
    ///      3 | remote port
    ///
    /// # Errors
    ///
    /// u8 array
    /// -------|---------
    /// offset | Contents
    /// =======|=========
    ///      0 | 1 indicating error
    ///      1 | 1 indicating error (duplicated)
    ///      2 | 1 indicating error (duplicated)
    ///      3 | 1 indicating error (duplicated)
    ///      4 | Error code
    StdTcpConnect = 30,

    /// Transmit data to the specified TCP Connection
    ///
    /// # Arguments
    ///
    /// The connection ID is OR-ed into the top 16 bits of the opcode. The payload
    /// is pointed to by the data buffer, and the `Valid` and Offset` flags indicate
    /// which regions inside the memory are valid.
    ///
    /// # Returns
    ///
    /// u32 array
    /// -------|---------
    /// offset | Contents
    /// =======|=========
    ///      0 | 0 if no error, 1 if error
    ///      1 | Number of bytes transferred, or code of the error
    ///
    StdTcpTx = 31,

    /// Receives data from the specified TCP Connection. The TCP connection number is
    /// passed in the upper 16 bits of the opcode, and the number of received bytes
    /// is returned as part of the `Valid` parameter. This is not blocking.
    StdTcpPeek = 32,

    /// Receives data from the specified TCP Connection. The TCP connection number is
    /// passed in the upper 16 bits of the opcode, and the number of received bytes
    /// is returned as part of the `Valid` parameter.
    StdTcpRx = 33,

    /// Close the TCP connection. The connection ID is specified in the upper 16 bits
    /// of the opcode. This may be any kind of message (scalar, blockingscalar, memory,
    /// etc.)
    StdTcpClose = 34,

    /// Get the current IP address
    StdGetAddress = 35,

    /// BlockingScalar call to get the current hop count of this connection
    StdGetTtl = 36,

    /// BlockingScalar call to set the maximum hop count of this connection
    StdSetTtl = 37,

    /// BlockingScalar call to get the NODELAY / "Nagle" value of this connection
    StdGetNodelay = 38,

    /// BlockingScalar call to set the NODELAY / "Nagle" value of this connection
    StdSetNodelay = 39,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone, Default)]
pub(crate) struct SsidList {
    /// IPC memory structures have to pre-allocate all their memory, but are always allocated in 4096-byte chunks.
    /// We could allocate up to maybe 100+ return values, but then we'd have to write a default initializer that
    /// covers a 64-length array. So, we limit at 32. <s>Thanks, Rust!</s> 32 APs should be enough for anyone, right?...
    pub(crate) list: [Option<SsidRecord>; 32],
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub enum XousServerId {
    /// A SID that is shared directly with the Net crate; a private, single-use SID for best security
    PrivateSid([u32; 4]),
    /// A name that needs to be looked up with XousNames. Easier to implement, but less secure as it requires
    /// an open connection slot that could be abused by other processes.
    ServerName(xous_ipc::String<64>),
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
pub(crate) enum WifiStateCallback {
    Update,
    Drop,
}
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct WifiStateSubscription {
    pub sid: [u32; 4],
    pub opcode: u32,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCallback {
    Ping,
    Drop,
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

#[repr(C)]
#[derive(Debug)]
pub enum NetError {
    // Ok = 0,
    Unaddressable = 1,
    SocketInUse = 2,
    // AccessDenied = 3,
    Invalid = 4,
    // Finished = 5,
    LibraryError = 6,
    // AlreadyUsed = 7,
    TimedOut = 8,
}

/////// a bunch of structures are re-derived here so we can infer `rkyv` traits on them
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetSocketAddr {
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
            _ => {
                panic!("Invalid IpAddress")
            }
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
            NetIpAddr::Ipv4([a, b, c, d]) => {
                IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(a, b, c, d))
            }
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
#[allow(dead_code)]
pub fn ipaddress_to_ipaddr(other: IpAddress) -> IpAddr {
    match other {
        IpAddress::Ipv4(ipv4) => {
            let octets = ipv4.0;
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        IpAddress::Ipv6(ipv6) => {
            let octets = ipv6.0;
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => unimplemented!(),
    }
}

impl fmt::Display for NetIpAddr {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetIpAddr::Ipv4(octets) => {
                // Fast Path: if there's no alignment stuff, write directly to the buffer
                if fmt.precision().is_none() && fmt.width().is_none() {
                    write!(
                        fmt,
                        "{}.{}.{}.{}",
                        octets[0], octets[1], octets[2], octets[3]
                    )
                } else {
                    const IPV4_BUF_LEN: usize = 15; // Long enough for the longest possible IPv4 address
                    let mut buf = [0u8; IPV4_BUF_LEN];
                    let mut buf_slice = &mut buf[..];

                    // Note: The call to write should never fail, hence the unwrap
                    write!(
                        buf_slice,
                        "{}.{}.{}.{}",
                        octets[0], octets[1], octets[2], octets[3]
                    )
                    .unwrap();
                    let len = IPV4_BUF_LEN - buf_slice.len();

                    // This unsafe is OK because we know what is being written to the buffer
                    let buf = unsafe { std::str::from_utf8_unchecked(&buf[..len]) };
                    fmt.pad(buf)
                }
            }
            NetIpAddr::Ipv6(ip) => ip.fmt(fmt),
        }
    }
}

impl fmt::Debug for NetIpAddr {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, fmt)
    }
}

/// This defines a Xous Scalar message endpoint.
/// This is useful for bridging the gap between a dedicated callback
/// server and a main loop. Note that both are within the same code
/// base and thus inherently trusted. It's assumed that you can create
/// a CID to bridge between the two, because you have free access
/// to the private SID.
///
/// The object is structured so that one can ask it to send a notification
/// at any time, but the notification is only issued if the
/// callback has been "hooked", that is, a CID/Op pair has been defined.
/// This way the notification can be optional, can also be unhookd once used.
///
/// This structure is not serializable or meant to be passed between memory
/// spaces: this is not the right object for passing messages from a remote server
/// in a potentially hostile foreign process into your local memory space.
/// See XousPrivateScalarHook for that function.
#[derive(Debug)]
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
            args: [None; 4],
        }
    }
    pub(crate) fn get(&self) -> (Option<xous::CID>, Option<usize>, [Option<usize>; 4]) {
        (self.cid, self.op, self.args)
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
                        self.args[0].unwrap_or_default(),
                        self.args[1].unwrap_or_default(),
                        self.args[2].unwrap_or_default(),
                        self.args[3].unwrap_or_default(),
                    ),
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
        log::trace!("custom args");
        if let Some(cid) = self.cid {
            if let Some(op) = self.op {
                log::trace!(
                    "ca: {} {} cust{:?} self{:?} 0:{}",
                    cid,
                    op,
                    custom,
                    self.args,
                    if let Some(b) = custom[0] {
                        b as usize
                    } else {
                        if let Some(a) = self.args[0] {
                            a
                        } else {
                            0
                        }
                    }
                );
                match xous::send_message(
                    cid,
                    xous::Message::new_scalar(
                        op,
                        if let Some(b) = custom[0] {
                            b as usize
                        } else {
                            if let Some(a) = self.args[0] {
                                a
                            } else {
                                0
                            }
                        },
                        if let Some(b) = custom[1] {
                            b as usize
                        } else {
                            if let Some(a) = self.args[1] {
                                a
                            } else {
                                0
                            }
                        },
                        if let Some(b) = custom[2] {
                            b as usize
                        } else {
                            if let Some(a) = self.args[2] {
                                a
                            } else {
                                0
                            }
                        },
                        if let Some(b) = custom[3] {
                            b as usize
                        } else {
                            if let Some(a) = self.args[3] {
                                a
                            } else {
                                0
                            }
                        },
                    ),
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

/// This is a "generic" object for registering a hook between a remote, potentially
/// untrusted server and my process. The SID filled into the hook should be a
/// "one time" (or perhaps better phrased as "single purpose") SID. In other words, it's
/// a SID created specifically to transact with this untrusted process, and nothing else;
/// you can receive as many messages as you like on it, but you should not use it for anything else.
///
/// This structure is rkyv-able, which means it can be serialized and sent between
/// process spaces.
///
/// The args field allow a scalar hook to define some extra metadata to send back and forth,
/// but they have no meaning in the case this is used for a Memory hook
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub struct XousPrivateServerHook {
    /// The SID shared here should be dedicated only to responding to this hook
    pub one_time_sid: [u32; 4],
    /// Opcode discriminant of the response message
    pub op: usize,
    /// Any args you want in the scalar; depends on the application
    pub args: [Option<usize>; 4],
}
