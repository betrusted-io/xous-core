#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use rkyv::Write;
use rkyv::Unarchive;
use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Point, Gid, Line, Rectangle, Circle, RoundedRectangle};
use log::info;

pub fn redraw(gam_cid: xous::CID) -> Result<(), xous::Error> {
    xous::send_message(gam_cid, api::Opcode::Redraw.into()).map(|_|())
}

/// this "posts" a textview -- it's not a "draw" as the update is neither guaranteed nor instantaneous
/// the GAM first has to check that the textview is allowed to be updated, and then it will decide when
/// the actual screen update is allowed
pub fn post_textview(gam_cid: xous::CID, tv: &mut TextView) -> Result<(), xous::Error> {
    tv.set_op(TextOp::Render);
    let rkyv_tv = api::Opcode::RenderTextView(*tv);
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("GAM_API: couldn't archive textview");
    let mut xous_buffer = writer.into_inner();

    xous_buffer.lend_mut(gam_cid, pos as u32).expect("GAM_API: RenderTextView operation failure");

    // recover the mutable values and mirror the ones we care about back into our local structure
    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::RenderTextView(result) = returned {
            let tvr: TextView = result.unarchive();
            tv.bounds_computed = tvr.bounds_computed;
            tv.cursor = tvr.cursor;
    } else {
        let tvr = returned.unarchive();
        info!("got {:?}", tvr);
        panic!("GAM_API: post_textview got a return value from the server that isn't expected or handled");
    }

    tv.set_op(TextOp::Nop);
    Ok(())
}

pub fn draw_line(gam_cid: xous::CID, gid: Gid, line: Line) -> Result<(), xous::Error> {
    let rkyv_tv = api::Opcode::RenderObject(
        GamObject {
            canvas: gid,
            obj: GamObjectType::Line(line),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("GAM_API: couldn't archive GamObject");
    let xous_buffer = writer.into_inner();
    xous_buffer.lend(gam_cid, pos as u32).expect("GAM_API: GamObject operation failure");
    Ok(())
}
pub fn draw_rectangle(gam_cid: xous::CID, gid: Gid, rect: Rectangle) -> Result<(), xous::Error> {
    let rkyv_tv = api::Opcode::RenderObject(
        GamObject {
            canvas: gid,
            obj: GamObjectType::Rect(rect),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("GAM_API: couldn't archive GamObject");
    let xous_buffer = writer.into_inner();
    xous_buffer.lend(gam_cid, pos as u32).expect("GAM_API: GamObject operation failure");
    Ok(())
}
pub fn draw_rouded_rectangle(gam_cid: xous::CID, gid: Gid, rr: RoundedRectangle) -> Result<(), xous::Error> {
    let rkyv_tv = api::Opcode::RenderObject(
        GamObject {
            canvas: gid,
            obj: GamObjectType::RoundRect(rr),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("GAM_API: couldn't archive GamObject");
    let xous_buffer = writer.into_inner();
    xous_buffer.lend(gam_cid, pos as u32).expect("GAM_API: GamObject operation failure");
    Ok(())
}
pub fn draw_circle(gam_cid: xous::CID, gid: Gid, circ: Circle) -> Result<(), xous::Error> {
    let rkyv_tv = api::Opcode::RenderObject(
        GamObject {
            canvas: gid,
            obj: GamObjectType::Circ(circ),
    });
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv_tv).expect("GAM_API: couldn't archive GamObject");
    let xous_buffer = writer.into_inner();
    xous_buffer.lend(gam_cid, pos as u32).expect("GAM_API: GamObject operation failure");
    Ok(())
}

pub fn get_canvas_bounds(gam_cid: xous::CID, gid: Gid) -> Result<Point, xous::Error> {
    let debug1 = false;
    if debug1{info!("GAM_API: get_canvas_bounds");}
    let response = xous::send_message(gam_cid, api::Opcode::GetCanvasBounds(gid).into())?;
    if let xous::Result::Scalar2(tl, br) = response {
        // note that the result should always be normalized so the rectangle's "tl" should be (0,0)
        if debug1{info!("GAM_API: tl:{}, br:{}", tl, br);}
        assert!(tl == 0, "GAM_API: api call returned non-zero top left for canvas bounds");
        Ok(br.into())
    } else {
        panic!("GAM_API: can't get canvas bounds")
    }
}

pub fn set_canvas_bounds_request(gam_cid: xous::CID, req: &mut SetCanvasBoundsRequest) -> Result<(), xous::Error> {
    let rkyv = api::Opcode::SetCanvasBounds((*req).clone());
    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GAM_API: couldn't archive SetCanvasBounds");
    let mut xous_buffer = writer.into_inner();
    xous_buffer.lend_mut(gam_cid, pos as u32).expect("GAM_API: SetCanvasBounds operation failure");

    // recover the mutable values and mirror the ones we care about back into our local structure
    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::SetCanvasBounds(result) = returned {
            let ret: SetCanvasBoundsRequest = result.unarchive();
            req.granted = ret.granted;
    } else {
        let ret = returned.unarchive();
        info!("got {:?}", ret);
        panic!("GAM_API: set_canvas_bounds_request view got a return value from the server that isn't expected or handled");
    }
    Ok(())
}

pub fn request_content_canvas(gam_cid: xous::CID, requestor_name: &str) -> Result<Gid, xous::Error> {
    let mut server = xous::String::<256>::new();
    use core::fmt::Write;
    write!(server, "{}", requestor_name).expect("GAM_API: couldn't write request_content_canvas server name");
    let req = ContentCanvasRequest {
        canvas: Gid::new([0,0,0,0]),
        servername: server,
    };
    let rkyv = api::Opcode::RequestContentCanvas(req);

    let mut writer = rkyv::ArchiveBuffer::new(xous::XousBuffer::new(4096));
    let pos = writer.archive(&rkyv).expect("GAM_API: couldn't archive RequestContentCanvas");
    let mut xous_buffer = writer.into_inner();
    xous_buffer.lend_mut(gam_cid, pos as u32).expect("GAM_API: RequestContentCanvas operation failure");

    // recover the mutable values and mirror the ones we care about back into our local structure
    let returned = unsafe { rkyv::archived_value::<api::Opcode>(xous_buffer.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::RequestContentCanvas(result) = returned {
        let ret: ContentCanvasRequest = result.unarchive();
        Ok(ret.canvas)
    } else {
        let ret = returned.unarchive();
        info!("got {:?}", ret);
        log::error!("GAM_API: request_content_canvas got a return value from the server that isn't expected or handled");
        Err(xous::Error::InternalError)
    }
}
