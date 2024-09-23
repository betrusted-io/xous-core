use rkyv::{Archive, Deserialize, Serialize};

use crate::api::*;

/// Scalar responses to pings have the following format:
/// arg1: bottom byte = NetPingCallback as below; top byte = DstUnreachable code as u8
/// arg2: remote IP address hint (IPv4 is full address; IPv6 is just bottom 4 bytes)
/// arg3: sequence number (if echo response) or top 4 bytes of an IPv6 address (if reporting timeout or drop)
/// arg4: elapsed time
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum NetPingCallback {
    /// echo response
    NoErr,
    /// timeout on a sequence number
    Timeout,
    /// dest unreachable
    Unreachable,
    /// An advisory message that one could drop the responding server, if it was spawned specifically for
    /// this use However, if the caller has grand plans to queue up more pings...then by all means, keep
    /// it around.
    Drop,
}

//////// Intra-crate Ping structures
#[derive(Debug, Archive, Serialize, Deserialize, Clone)]
pub(crate) struct NetPingPacket {
    /// the address we are pinging
    pub endpoint: NetIpAddr,
    /// the server for our callback when the pong arrives
    pub server: XousServerId,
    /// the opcode ID for the callback
    pub return_opcode: usize,
    /// a response from the Net crate informing if the send was successful
    pub sent_ok: Option<bool>,
}
