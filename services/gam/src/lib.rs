#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use rkyv::Write;
use rkyv::Unarchive;
use graphics_server::api::{TextOp, TextView, TextViewResult};

use graphics_server::api::{Rectangle, Point, Gid, TextBounds};
use log::{error, info};

/// this "posts" a textview -- it's not a "draw" as the update is neither guaranteed nor instantaneous
/// the GAM first has to check that the textview is allowed to be updated, and then it will decide when
/// the actual screen update is allowed
pub fn post_textview(gam_cid: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    tv.set_op(TextOp::Render);
    let mut rkyv_tv = api::Opcode::RenderTextView(*tv);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("couldn't archive textview");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(gam_cid, pos as u32).expect("RenderTextView operation failure");

    // recover the mutable values and mirror the ones we care about back into our local structure
    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::RenderTextView(result) = returned {
            let tvr: TextView = result.unarchive();
            tv.bounds_computed = tvr.bounds_computed;
            tv.cursor = tvr.cursor;
    } else {
        let tvr = returned.unarchive();
        info!("got {:?}", tvr);
        panic!("post_textview got a return value from the server that isn't expected or handled");
    }

    tv.set_op(TextOp::Nop);
    Ok(())
}