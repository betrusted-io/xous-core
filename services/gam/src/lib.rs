#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use xous::ipc::*;
use core::fmt::Write;
use graphics_server::api::{TextOp, TextView};

pub fn draw_textview(gam_cid: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    let mut sendable_tv = Sendable::new(tv).expect("can't create sendable textview");
    sendable_tv.set_op(TextOp::Render);
    sendable_tv.lend_mut(gam_cid, sendable_tv.get_op().into()).expect("draw_textview operation failure");

    sendable_tv.set_op(TextOp::Nop);
    Ok(())
}