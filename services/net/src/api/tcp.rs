use com::api::NET_MTU;
use rkyv::{Archive, Deserialize, Serialize};

use crate::api::*;

pub(crate) const TCP_BUFFER_SIZE: usize = NET_MTU;

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetTcpManage {
    pub(crate) cb_sid: [u32; 4],
    pub(crate) ip_addr: NetIpAddr,
    pub(crate) remote_port: u16,
    pub(crate) local_port: Option<u16>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) keepalive_ms: Option<u64>,
    pub(crate) result: Option<NetMemResponse>,
    // below is for management functions
    pub(crate) mgmt_code: Option<TcpMgmtCode>,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetTcpListen {
    pub(crate) cb_sid: [u32; 4],
    pub(crate) local_port: u16,
    pub(crate) result: Option<NetMemResponse>,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetTcpListenCallback {
    pub(crate) ip_addr: NetIpAddr,
    pub(crate) remote_port: u16,
    pub(crate) local_port: u16,
}

#[allow(dead_code)]
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetTcpCallback {
    RxData,
    ListenerActive,
    Drop,
}

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
pub(crate) struct NetTcpResponse {
    pub data: [u8; TCP_BUFFER_SIZE],
    pub len: u16,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetTcpTransmit {
    pub remote_addr: NetIpAddr,
    pub remote_port: u16,
    // this was given back in the connect, we need this to distinguish between multiple connections to the
    // same server
    pub local_port: u16,
    // this is changed to the number of bytes actually sent on return
    pub len: u16,
    pub data: [u8; TCP_BUFFER_SIZE],
    pub result: Option<NetMemResponse>,
}
