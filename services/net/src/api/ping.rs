use rkyv::{Archive, Deserialize, Serialize};
use crate::api::*;

// anything bigger than this just gets truncated.
// do we really care to respond to naughty hosts?
pub(crate) const PING_MAX_PKT_LEN: usize = 256;

//////// Intra-crate Ping structures
#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetPingPacket {
    pub len: u32,
    pub data: [u8; PING_MAX_PKT_LEN],
    pub endpoint: NetIpAddr,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone)]
pub(crate) struct NetPingResponsePacket {
    pub len: u32,
    pub data: [u8; PING_MAX_PKT_LEN],
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum NetPingCallback {
    RxData,
    SrcAddr,
    CheckTimeout,
    Drop,
}
