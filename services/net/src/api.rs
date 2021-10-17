use rkyv::{Archive, Deserialize, Serialize};
use std::net::SocketAddr;
use smoltcp::wire::IpAddress;

pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    UdpBind,
    UdpClose,

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
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetCallback {
    Ping,
    Drop,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetUdpCallback {
    RxData,
    Drop,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) enum NetIpAddr {
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

pub(crate) const UDP_RESPONSE_MAX_LEN: usize = 1800;
/// The data field for a UDP response is limited to less than the theoretical
/// size of 64k. While UDP allows for a 64k packet, it seems no protoctols
/// in practice utilize such a length (about 512 bytes is the biggest), due
/// to MTU limitations downstream. Within Xous, memory is shared on a page-basis,
/// which is 4096 bytes, so the cost to share a page is almost the same regardless
/// of its size, as long as it is smaller than 4096 bytes. Hence, the number
/// 1800 bytes is picked to be a bit larger than our wifi MTU, but small
/// enough to fit in a page of RAM. Why not make it even bigger? Mainly to save
/// on the cost to repeatedly zeroize parts of RAM that are never used.
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetUdpResponse {
    pub endpoint_ip_addr: NetIpAddr,
    pub len: u16,
    pub endpoint_port: u16,
    pub data: [u8; UDP_RESPONSE_MAX_LEN],
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32,  // ID of the scalar message to send through (e.g. the discriminant of the Enum on the caller's side API)
    pub cid: xous::CID,   // caller-side connection ID for the scalar message to route to. Created by the caller before hooking.
    pub token: Option<[u32; 4]>, // 128-bit random token used to identify a connection to the net crate
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetUdpBind {
    pub(crate) cb_sid: [u32; 4],
    pub(crate) ip_addr: NetIpAddr,
    pub(crate) port: u16,
    pub(crate) max_payload: Option<u16>, // defaults to MTU if not specified
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) enum NetMemResponse {
    Ok,
    OutOfMemory,
    SocketInUse,
    AccessDenied,
    Invalid,
}

/// This defines a Xous Scalar message endpoint. Used for defining
/// notification messages for incoming packets, for main loops that want to be
/// able to process both events from Xous and from the network.
pub struct XousScalarEndpoint {
    cid: Option<xous::CID>,
    op: Option<usize>,
    args: [Option<usize>; 4],
}
#[allow(dead_code)]
impl XousScalarEndpoint {
    pub fn new() -> Self {
        XousScalarEndpoint {
            cid: None,
            op: None,
            args: [None; 4]
        }
    }
    pub fn set(&mut self, cid: xous::CID, op: usize, args: [Option<usize>; 4]) {
        self.cid = Some(cid);
        self.op = Some(op);
        self.args = args;
    }
    pub fn clear(&mut self) {
        self.cid = None;
        self.op = None;
        self.args = [None; 4];
    }
    pub fn is_set(&self) -> bool {
        self.cid.is_some() && self.op.is_some()
    }
    pub fn notify(&mut self) {
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
}