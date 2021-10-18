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

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetUdpCallback {
    RxData,
    Drop,
}

