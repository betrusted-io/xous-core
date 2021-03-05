#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

mod api;
use api::*;
mod status;
use status::*;
mod canvas;
use canvas::*;

use blitstr_ref as blitstr;
use blitstr::GlyphStyle;
use graphics_server::*;
use ime_plugin_api::ImeFrontEndApi;

use log::info;

use core::convert::TryFrom;

use heapless::FnvIndexMap;
use heapless::consts::*;

use rkyv::{Unarchive, archived_value, archived_value_mut};
use core::pin::Pin;

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
        255, trng_conn, None
    ).expect("GAM: couldn't create password canvas");
    canvases.insert(password_canvas.gid(), password_canvas).expect("GAM: can't store password canvas");

    let menu_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        255, trng_conn, None
    ).expect("GAM: couldn't create menu canvas");
    canvases.insert(menu_canvas.gid(), menu_canvas).expect("GAM: can't store menu canvas");

    let alert_canvas = Canvas::new(
        Rectangle::new_coords(-1, -1, -1, -1),
        255, trng_conn, None
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
    let margin = 4;

    // allocate canvases in structures, and record their GID for future reference
    let status_canvas = Canvas::new(
        Rectangle::new_coords(0, 0, screensize.x, small_height),
        255, trng_conn, None
    ).expect("GAM: couldn't create status canvas");
    canvases.insert(status_canvas.gid(), status_canvas).expect("GAM: can't store status canvus");

    let predictive_canvas = Canvas::new(
        Rectangle::new_coords(0, screensize.y - regular_height - margin*2, screensize.x, screensize.y),
        254,
        trng_conn, None
    ).expect("GAM: couldn't create predictive text canvas");
    canvases.insert(predictive_canvas.gid(), predictive_canvas).expect("GAM: couldn't store predictive canvas");

    let input_canvas = Canvas::new(
        Rectangle::new_v_stack(predictive_canvas.clip_rect(), -regular_height - margin*2),
        254, trng_conn, None
    ).expect("GAM: couldn't create input text canvas");
    canvases.insert(input_canvas.gid(), input_canvas).expect("GAM: couldn't store input canvas");

    let content_canvas = Canvas::new(
        Rectangle::new_v_span(status_canvas.clip_rect(), input_canvas.clip_rect()),
        128, trng_conn, None
    ).expect("GAM: couldn't create content canvas");
    canvases.insert(content_canvas.gid(), content_canvas).expect("GAM: can't store content canvas");

    Ok(ChatLayout {
        status: status_canvas.gid(),
        content: content_canvas.gid(),
        predictive: predictive_canvas.gid(),
        input: input_canvas.gid(),
    })
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;  // debug level 1 - most general level
    log_server::init_wait().unwrap();
    info!("GAM: my PID is {}", xous::process::id());

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
    // the status bar is a trusted element managed by the OS, and we are chosing to domicile this in the GAM process for now
    xous::create_thread_simple(status_thread, chatlayout.status.gid()).expect("GAM: couldn't create status thread");

    // connect to the IME front end, and set its canvas
    info!("GAM: acquiring connection to IMEF...");
    let imef_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_FRONT).expect("GAM: can't connect to the IME front end");
    let imef = ime_plugin_api::ImeFrontEnd{ connection: Some(imef_conn), };
    imef.set_input_canvas(chatlayout.input).expect("GAM: couldn't set IMEF input canvas");
    imef.set_prediction_canvas(chatlayout.predictive).expect("GAM: couldn't set IMEF prediction canvas");

    // ASSUME: shell is our default application, so set a default predictor of Shell
    imef.set_predictor(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("GAM: couldn't set IMEF prediction to shell");
    // NOTE: all three API calls (set_input_canvas, set_prediction_canvas, set_predictor) are mandatory for IMEF initialization

    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    info!("GAM: entering main loop");
    loop {
        let maybe_env = xous::try_receive_message(gam_sid).unwrap();
        if debug1 { if maybe_env.is_some() {info!("GAM: Message: {:?}", maybe_env);} }
        match maybe_env {
            Some(envelope) => {
                if debug1 {info!("GAM: Message: {:?}", envelope); }
                if let Ok(opcode) = Opcode::try_from(&envelope.body) {
                    if debug1 {info!("GAM: Opcode: {:?}", opcode);}
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
                        Opcode::GetCanvasBounds(gid) => {
                            match canvases.get(&gid) {
                                Some(c) => {
                                    let mut rect = c.clip_rect();
                                    rect.normalize(); // normalize to 0,0 coordinates
                                    xous::return_scalar2(envelope.sender,
                                        rect.tl.into(),
                                        rect.br.into(),
                                    ).expect("GAM: couldn't return canvas bounds");
                                },
                                None => info!("GAM: attempt to get bounds on bogus canvas, ignored."),
                            }
                        }
                        _ => todo!("GAM: opcode not yet implemented"),
                    }
                } else if let xous::Message::MutableBorrow(m) = &envelope.body {
                    let mut buf = unsafe { xous::XousBuffer::from_memory_message(m) };
                    let value = unsafe {
                        archived_value_mut::<api::Opcode>(Pin::new(buf.as_mut()), m.id as usize)
                    };
                    match &*value {
                        rkyv::Archived::<api::Opcode>::RenderTextView(rtv) => {
                            let mut tv = rtv.unarchive();
                            if debug1{info!("GAM: rendertextview {:?}", tv);}
                            match tv.get_op() {
                                TextOp::Nop => (),
                                TextOp::Render | TextOp::ComputeBounds => {
                                    if debug1{info!("GAM: render request for {:?}", tv);}
                                    if tv.get_op() == TextOp::ComputeBounds {
                                        tv.dry_run = true;
                                    } else {
                                        tv.dry_run = false;
                                    }

                                    if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                                        // first, figure out if we should even be drawing to this canvas.
                                        if canvas.is_drawable() {
                                            // set the clip rectangle according to the canvas' location
                                            tv.clip_rect = Some(canvas.clip_rect().into());

                                            // you have to clone the tv object, because if you don't the same block of
                                            // memory gets passed on to the graphics_server(). Which is efficient, but,
                                            // the call will automatically Drop() the memory, which causes a panic when
                                            // this routine returns.
                                            let mut tv_clone = tv.clone();
                                            // issue the draw command
                                            graphics_server::draw_textview(gfx_conn, &mut tv_clone).expect("GAM: text view draw could not complete.");
                                            // copy back the fields that we want to be mutable
                                            if debug1{info!("GAM: got computed cursor of {:?}", tv_clone.cursor);}
                                            tv.cursor = tv_clone.cursor;
                                            tv.bounds_computed = tv_clone.bounds_computed;

                                            // pack our data back into the buffer to return
                                            use rkyv::Write;
                                            let mut writer = rkyv::ArchiveBuffer::new(buf);
                                            writer.archive(&api::Opcode::RenderTextView(tv)).expect("GAM: couldn't re-archive return value");
                                        } else {
                                            info!("GAM: attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored.");
                                        }
                                    } else {
                                        info!("GAM: bogus GID in TextView, not doing anything in response to draw request.");
                                        // silently fail if a bogus Gid is given???
                                    }
                                },
                            };
                        },
                        _ => panic!("GAM: invalid mutable borrow message"),
                    };
                } else if let xous::Message::Borrow(m) = &envelope.body {
                    let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
                    let bytes = Pin::new(buf.as_ref());
                    let value = unsafe {
                        archived_value::<api::Opcode>(&bytes, m.id as usize)
                    };
                    match &*value {
                        rkyv::Archived::<api::Opcode>::RenderObject(rtv) => {
                            let obj: GamObject = rtv.unarchive();
                            if debug1{info!("GAM: renderobject {:?}", obj);}
                            if let Some(canvas) = canvases.get_mut(&obj.canvas) {
                                // first, figure out if we should even be drawing to this canvas.
                                if canvas.is_drawable() {
                                    match obj.obj {
                                        GamObjectType::Line(mut line) => {
                                            line.translate(canvas.clip_rect().tl);
                                            line.translate(canvas.pan_offset());
                                            graphics_server::draw_line_clipped(gfx_conn,
                                                line,
                                                canvas.clip_rect(),
                                            ).expect("GAM: couldn't draw line");
                                        },
                                        GamObjectType::Circ(mut circ) => {
                                            circ.translate(canvas.clip_rect().tl);
                                            circ.translate(canvas.pan_offset());
                                            graphics_server::draw_circle_clipped(gfx_conn,
                                                circ,
                                                canvas.clip_rect(),
                                            ).expect("GAM: couldn't draw circle");
                                        },
                                        GamObjectType::Rect(mut rect) => {
                                            rect.translate(canvas.clip_rect().tl);
                                            rect.translate(canvas.pan_offset());
                                            graphics_server::draw_rectangle_clipped(gfx_conn,
                                                rect,
                                                canvas.clip_rect(),
                                            ).expect("GAM: couldn't draw rectangle");
                                        },
                                        GamObjectType::RoundRect(mut rr) => {
                                            rr.translate(canvas.clip_rect().tl);
                                            rr.translate(canvas.pan_offset());
                                            graphics_server::draw_rounded_rectangle_clipped(gfx_conn,
                                                rr,
                                                canvas.clip_rect(),
                                            ).expect("GAM: couldn't draw rounded rectangle");
                                        }
                                    }
                                } else {
                                    info!("GAM: attempt to draw Object on non-drawable canvas. Not fatal, but request ignored.");
                                }
                            } else {
                                info!("GAM: bogus GID in Object, not doing anything in response to draw request.");
                            }
                            if debug1{info!("GAM: leaving RenderObject");}
                        },
                        _ => panic!("GAM: invalid borrow message"),
                    };
                } else {
                    panic!("GAM: unhandled message {:?}", envelope);
                }
                // /*
            },
            _ => xous::yield_slice(),
            // envelope implements Drop(), which includes a call to syscall::return_memory(self.sender, message.buf)
        }

        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            if elapsed_time - last_time > 33 {  // rate limit updates to 30fps
                last_time = elapsed_time;

                // TODO: enhance this update with a methods that:
                //  1. deface dirty areas that require defacing
                //  2. update the drawn dirty bits indicating things have been drawn, as appropriate
                graphics_server::flush(gfx_conn).expect("GAM: couldn't flush buffer to screen");
            }
        }
    }
}
