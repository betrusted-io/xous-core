#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use xous::ipc::*;
use core::fmt::Write;
use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Rectangle, Point, Gid, TextBounds};
use log::{error, info};


/// this "posts" a textview -- it's not a "draw" as the update is neither guaranteed nor instantaneous
/// the GAM first has to check that the textview is allowed to be updated, and then it will decide when
/// the actual screen update is allowed
pub fn post_textview(gam_cid: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    let mut tv_backup = TextView::new(Gid::new([0,0,0,0]), 0,
        TextBounds::BoundingBox(Rectangle::new(Point::new(0, 0), Point::new(0, 0))));
    tv_backup.populate_from(&tv);
    info!("tv_backup: {:?}", tv_backup);

    let mut sendable_tv = Sendable::new(tv).expect("can't create sendable textview");
    sendable_tv.populate_from(&tv_backup);
    info!("sendable_tv: {:?}", sendable_tv);

    sendable_tv.set_op(TextOp::Render);
    sendable_tv.cursor.pt.x = 37;
    sendable_tv.cursor.pt.y = 5;

    info!("sendable_tv before lend: {:?}", sendable_tv);
    sendable_tv.lend_mut(gam_cid, sendable_tv.get_op().into()).expect("draw_textview operation failure");

    sendable_tv.set_op(TextOp::Nop);
    Ok(())
}