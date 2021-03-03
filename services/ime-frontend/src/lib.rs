#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use xous::{CID, send_message};

pub fn set_canvas(cid: CID, g: graphics_server::Gid) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::SetCanvas(g).into())?;
}
