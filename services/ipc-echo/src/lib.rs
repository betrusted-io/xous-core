//! Wire types for the IPC test echo server. Shared between `ipc-echo` (the
//! server) and `ipc-tests` (the client/runner), mirroring how the real service
//! API crates publish their rkyv payloads.

use rkyv::{Archive, Deserialize, Serialize};

pub const SERVER_NAME: &str = "ipc-echo";
pub const OP_ECHO: usize = 0x100;

/// Payload for one echo round-trip. `data` and `label` are heap-backed, so a
/// large payload exercises the probe-sized multi-page buffer path end to end:
/// client serializes -> kernel RW-lends N pages -> server deserializes ->
/// server replaces with a (differently sized) response -> client reads it back
/// through the returned offset.
#[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
pub struct EchoPayload {
    pub tag: u32,
    pub data: Vec<u8>,
    pub label: String,
}
