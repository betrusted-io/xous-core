pub(crate) mod udp;
pub(crate) use udp::*;

use rkyv::{Archive, Deserialize, Serialize};
use std::net::SocketAddr;
use smoltcp::wire::IpAddress;

pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    UdpBind,
    UdpClose,
    UdpTx,

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
