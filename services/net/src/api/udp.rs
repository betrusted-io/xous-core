use rkyv::{Archive, Deserialize, Serialize};
use crate::api::*;

//////// Intra-crate UDP structures
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
pub(crate) struct NetUdpTransmit {
    pub dest_socket: Option<NetSocketAddr>,
    /// local_port is the identifier for the socket handle, it must be specified
    pub local_port: u16,
    pub len: u16,
    pub data: [u8; UDP_RESPONSE_MAX_LEN],
}

/* not used as the connect state is kept on the caller's side
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetUdpConnect {
    pub dest_socket: NetSocketAddr,
    /// local_port is the identifier for the socket handle, it must be specified
    pub local_port: u16,
}*/

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetUdpBind {
    pub(crate) cb_sid: [u32; 4],
    pub(crate) ip_addr: NetIpAddr,
    pub(crate) port: u16,
    pub(crate) max_payload: Option<u16>, // defaults to MTU if not specified
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetUdpCallback {
    RxData,
    Drop,
}

