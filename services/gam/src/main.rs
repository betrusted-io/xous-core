#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

mod api;
use api::*;
mod status;
use status::*;
mod canvas;
use canvas::*;

use blitstr::{Cursor, GlyphStyle};
use com::*;
use core::fmt::Write;
use graphics_server::*;

use log::{error, info};
use xous::String;

use core::convert::TryFrom;

use heapless::binary_heap::{BinaryHeap, Max};
use heapless::FnvIndexMap;
use heapless::consts::*;

// GIDs of canvases that are used the "Chat" layout.
struct ChatLayout {
    // a set of GIDs to track the elements of the chat layout
    pub status: Gid,
    pub content: Gid,
    pub predictive: Gid,
    pub input: Gid,
}

// remember GIDs of the canvases for modal pop-up boxes
struct ModalCanvases {
    pub password: Gid,
    pub menu: Gid,
    pub alert: Gid,
}

fn add_modal_layout(trng_conn: xous::CID, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ModalCanvases, xous::Error> {
    let password_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        0, trng_conn, None
    ).expect("GAM: couldn't create password canvas");
    canvases.insert(password_canvas.gid(), password_canvas).expect("GAM: can't store password canvas");

    let menu_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        0, trng_conn, None
    ).expect("GAM: couldn't create menu canvas");
    canvases.insert(menu_canvas.gid(), menu_canvas).expect("GAM: can't store menu canvas");

    let alert_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        0, trng_conn, None
    ).expect("GAM: couldn't create alert canvas");
    canvases.insert(alert_canvas.gid(), alert_canvas).expect("GAM: can't store alert canvas");

    Ok(ModalCanvases {
        password: password_canvas.gid(),
        menu: menu_canvas.gid(),
        alert: alert_canvas.gid(),
    })
}

fn add_chat_layout(gfx_conn: xous::CID, trng_conn: xous::CID, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ChatLayout, xous::Error> {
    let screensize = graphics_server::screen_size(gfx_conn).expect("GAM: Couldn't get screen size");
    // get the height of various text regions to compute the layout
    let small_height: i16 = graphics_server::glyph_height_hint(gfx_conn, GlyphStyle::Small).expect("GAM: couldn't get glyph height") as i16;
    let regular_height: i16 = graphics_server::glyph_height_hint(gfx_conn, GlyphStyle::Regular).expect("GAM: couldn't get glyph height") as i16;

    // allocate canvases in structures, and record their GID for future reference
    let status_canvas = Canvas::new(
        Rectangle::new_coords(0, 0, screensize.x, small_height),
        0, trng_conn, None
    ).expect("GAM: couldn't create status canvas");
    canvases.insert(status_canvas.gid(), status_canvas).expect("GAM: can't store status canvus");

    let predictive_canvas = Canvas::new(
        Rectangle::new_coords(0, screensize.y - regular_height, screensize.x, screensize.y),
        1,
        trng_conn, None
    ).expect("GAM: couldn't create predictive text canvas");
    canvases.insert(predictive_canvas.gid(), predictive_canvas).expect("GAM: couldn't store predictive canvas");

    let input_canvas = Canvas::new(
        Rectangle::new_v_stack(predictive_canvas.clip_rect(), -regular_height),
        1, trng_conn, None
    ).expect("GAM: couldn't create input text canvas");
    canvases.insert(input_canvas.gid(), input_canvas).expect("GAM: couldn't store input canvas");

    let content_canvas = Canvas::new(
        Rectangle::new_v_span(status_canvas.clip_rect(), input_canvas.clip_rect()),
        2, trng_conn, None
    ).expect("GAM: couldn't create content canvas");
    canvases.insert(content_canvas.gid(), content_canvas).expect("GAM: can't store content canvas");

    Ok(ChatLayout {
        status: status_canvas.gid(),
        content: content_canvas.gid(),
        predictive: predictive_canvas.gid(),
        input: input_canvas.gid(),
    })
}

fn tv_draw(gfx_conn: xous::CID, trng_conn: xous::CID, canvases: &mut FnvIndexMap<Gid, Canvas, U32>, tv: &mut TextView) -> Result<(), xous::Error> {
    /*
       1. figure out text bounds
       2. clear background, if requested
       3. draw surrounding rectangle, if requested
       4. draw text
     */

    if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
        // first, figure out if we should even be drawing to this canvas.
        if canvas.is_drawable() {
            // figure out text bounds: figure out how wide our text is, to start with.
            graphics_server::set_string_clipping(gfx_conn, canvas.clip_rect().into()).expect("GAM: can't set clipping");

        } else {
            info!("GAM: attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored.");
            return Ok(())
        }


    } else {
        info!("GAM: bogus GID in TextView, not doing anything in response to draw request.");
        return Ok(()) // silently fail if a bogus Gid is given???
    }

    Ok(())

}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();

    let gam_sid = xous_names::register_name(xous::names::SERVER_NAME_GAM).expect("GAM: can't register server");
    info!("GAM: starting up...");

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    let gfx_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_GFX).expect("GAM: can't connect to COM");
    let trng_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_TRNG).expect("GAM: can't connect to TRNG");

    let screensize = graphics_server::screen_size(gfx_conn).expect("GAM: Couldn't get screen size");

    // a map of canvases accessable by Gid
    let mut canvases: FnvIndexMap<Gid, Canvas, U32> = FnvIndexMap::new();
    let modallayout = add_modal_layout(trng_conn, &mut canvases).expect("GAM: can't add modal layouts");
    let chatlayout = add_chat_layout(gfx_conn, trng_conn, &mut canvases).expect("GAM: couldn't create chat layout");

    // now that all the initial canvases have been allocated, compute what canvases are drawable
    // this _replaces_ the original canvas structure, to avoid complications of tracking mutable references through compound data structures
    // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
    canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));

    // make a thread to manage the status bar
    xous::create_thread_simple(status_thread, chatlayout.status.gid()).expect("GAM: couldn't create status thread");

    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    info!("GAM: entering main loop");
    loop {
        let maybe_env = xous::try_receive_message(gam_sid).unwrap();
        match maybe_env {
            Some(envelope) => {
                info!("GAM: Message: {:?}", envelope);
                if let Ok(opcode) = Opcode::try_from(&envelope.body) {
                    match opcode {
                        Opcode::ClearCanvas(gid) => {
                            match canvases.get(&gid) {
                                Some(c) => {
                                    let mut rect = c.clip_rect();
                                    rect.style = DrawStyle {
                                        fill_color: Some(PixelColor::Light),
                                        stroke_color: Some(PixelColor::Light),
                                        stroke_width: 1,
                                    };
                                    graphics_server::draw_rectangle(gfx_conn, rect).expect("GAM: can't clear canvas");
                                },
                                None => info!("GAM: attempt to clear bogus canvas, ignored."),
                            }
                        },
                        _ => todo!("GAM: opcode not yet implemented"),
                    }
                } else if let xous::Message::MutableBorrow(m) = &envelope.body {
                    match TextOp::from(m.id) {
                        TextOp::Nop => (),
                        TextOp::Render => {
                            let mut tv: &mut TextView = unsafe {
                                &mut *(m.buf.as_mut_ptr() as *mut TextView)
                            };
                            info!("GAM: render request for {:?}", tv);
                            tv_draw(gfx_conn, trng_conn, &mut canvases, &mut tv).expect("GAM: can't render TextView");
                        },
                        TextOp::ComputeBounds => {

                        },
                    }
                }
            }
            _ => xous::yield_slice(),
        }

        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 33 {  // rate limit updates to 30fps
                graphics_server::flush(gfx_conn).expect("GAM: couldn't flush buffer to screen");
            }
        }
    }
}
