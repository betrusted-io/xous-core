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

use content_plugin_api::{ContentCanvasConnection, ContentCanvasApi};

use log::info;

use core::convert::TryFrom;

use heapless::FnvIndexMap;
use heapless::consts::*;

use rkyv::{archived_value, archived_value_mut};
use rkyv::Deserialize;
use rkyv::ser::Serializer;
use core::pin::Pin;

#[derive(Debug)]
// GIDs of canvases that are used the "Chat" layout.
struct ChatLayout {
    // a set of GIDs to track the elements of the chat layout
    pub status: Gid,
    pub content: Gid,
    pub predictive: Gid,
    pub input: Gid,

    // my internal bookkeeping records. Allow input area to grow into content area
    min_content_height: i16,
    min_input_height: i16,
    gfx_conn: xous::CID,
}
impl ChatLayout {
    pub fn init(gfx_conn: xous::CID, trng_conn: xous::CID, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ChatLayout, xous::Error> {
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

        let min_input_height = regular_height + margin*2;
        let input_canvas = Canvas::new(
            Rectangle::new_v_stack(predictive_canvas.clip_rect(), -min_input_height),
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
            min_content_height: 64,
            min_input_height,
            gfx_conn,
        })
    }
    pub fn clear(&self, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<(), xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("GAM: couldn't find input canvas");
        let content_canvas = canvases.get(&self.content).expect("GAM: couldn't find content canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("GAM: couldn't find predictive canvas");
        let status_canvas = canvases.get(&self.status).expect("GAM: couldn't find status canvas");

        let mut rect = status_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        graphics_server::draw_rectangle(self.gfx_conn, rect).expect("GAM: can't clear canvas");

        let mut rect = content_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        graphics_server::draw_rectangle(self.gfx_conn, rect).expect("GAM: can't clear canvas");

        let mut rect = predictive_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        graphics_server::draw_rectangle(self.gfx_conn, rect).expect("GAM: can't clear canvas");

        let mut rect = input_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        graphics_server::draw_rectangle(self.gfx_conn, rect).expect("GAM: can't clear canvas");
        Ok(())
    }
    pub fn resize_input(&mut self, new_height: i16, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<Point, xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("GAM: couldn't find input canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("GAM: couldn't find predictive canvas");
        let status_canvas = canvases.get(&self.status).expect("GAM: couldn't find status canvas");

        let height: i16 = if new_height < self.min_input_height {
            self.min_input_height
        } else {
            new_height
        };
        let mut new_input_rect = Rectangle::new_v_stack(predictive_canvas.clip_rect(), -height);
        let mut new_content_rect = Rectangle::new_v_span(status_canvas.clip_rect(), new_input_rect);
        if (new_content_rect.br.y - new_content_rect.tl.y) > self.min_content_height {
            {
                let input_canvas_mut = canvases.get_mut(&self.input).expect("GAM: couldn't find input canvas");
                input_canvas_mut.set_clip(new_input_rect);
                new_input_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                graphics_server::draw_rectangle(self.gfx_conn, new_input_rect).expect("GAM: can't clear canvas");
                    }
            {
                let content_canvas_mut = canvases.get_mut(&self.content).expect("GAM: couldn't find content canvas");
                content_canvas_mut.set_clip(new_content_rect);
                new_content_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                graphics_server::draw_rectangle(self.gfx_conn, new_content_rect).expect("GAM: can't clear canvas");
            }
            // we resized to this new height
            Ok(new_content_rect.br)
        } else {
            // we didn't resize anything, height unchanged
            Ok(input_canvas.clip_rect().br)
        }
    }
}

// remember GIDs of the canvases for modal pop-up boxes
struct ModalCanvases {
    pub password: Gid,
    pub menu: Gid,
    pub alert: Gid,
}
impl ModalCanvases {
    fn init(trng_conn: xous::CID, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ModalCanvases, xous::Error> {
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
}


#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;  // debug level 1 - most general level
    let debugc = true;
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
    // let modallayout = ModalCanvases::init(trng_conn, &mut canvases).expect("GAM: can't add modal layouts");
    let mut chatlayout = ChatLayout::init(gfx_conn, trng_conn, &mut canvases).expect("GAM: couldn't create chat layout");
    chatlayout.clear(&mut canvases).expect("GAM: couldn't clear initial chatlayout");

    // now that all the initial canvases have been allocated, compute what canvases are drawable
    // this _replaces_ the original canvas structure, to avoid complications of tracking mutable references through compound data structures
    // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
    canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));

    // connect to the IME front end, and set its canvas
    info!("GAM: acquiring connection to IMEF...");
    let imef_conn = xous_names::request_connection_blocking(xous::names::SERVER_NAME_IME_FRONT).expect("GAM: can't connect to the IME front end");
    let imef = ime_plugin_api::ImeFrontEnd{ connection: Some(imef_conn), };
    imef.set_input_canvas(chatlayout.input).expect("GAM: couldn't set IMEF input canvas");
    imef.set_prediction_canvas(chatlayout.predictive).expect("GAM: couldn't set IMEF prediction canvas");

    // ASSUME: shell is our default application, so set a default predictor of Shell
    imef.set_predictor(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("GAM: couldn't set IMEF prediction to shell");
    // NOTE: all three API calls (set_input_canvas, set_prediction_canvas, set_predictor) are mandatory for IMEF initialization

    // no content canvas initially, but keep a placeholder for one
    let mut ccc: ContentCanvasConnection = ContentCanvasConnection{connection: None};

    if debugc{info!("GAM: chatlayout made st {:?} co {:?} pr {:?} in {:?}", chatlayout.status, chatlayout.content, chatlayout.predictive, chatlayout.input);}
    // make a thread to manage the status bar -- this needs to start after the IMEF is initialized
    // the status bar is a trusted element managed by the OS, and we are chosing to domicile this in the GAM process for now
    let gid = chatlayout.status.gid();
    xous::create_thread_4(status_thread, gid[0] as _, gid[1] as _, gid[2] as _, gid[3] as _).expect("GAM: couldn't create status thread");

    let mut powerdown_requested = false;
    let mut last_time: u64 = ticktimer_server::elapsed_ms(ticktimer_conn).unwrap();
    info!("GAM: entering main loop");
    loop {
        let envelope = xous::receive_message(gam_sid).unwrap();
        if debug1 { info!("GAM: Message: {:?}", envelope);}
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            if debug1 {info!("GAM: Opcode: {:?}", opcode);}
            match opcode {
                Opcode::ClearCanvas(gid) => {
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
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
                        None => info!("GAM: attempt to get bounds on bogus canvas gid {:?}, {:?} ignored.", gid, envelope),
                    }
                },
                Opcode::PowerDownRequest => {
                    powerdown_requested = true;
                    graphics_server::draw_sleepscreen(gfx_conn).expect("GAM: couldn't draw sleep screen");
                    // a screen flush is part of the draw_sleepscreen abstraction
                    xous::return_scalar(envelope.sender, 1).expect("GAM: couldn't confirm power down UI request");
                },
                Opcode::Redraw => {
                    if powerdown_requested {
                        continue; // don't allow any redraws if a powerdown is requested
                    }
                    if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
                        if elapsed_time - last_time > 33 {  // rate limit updates, no point in going faster than the eye can see
                            last_time = elapsed_time;

                            deface(gfx_conn, &mut canvases);
                            graphics_server::flush(gfx_conn).expect("GAM: couldn't flush buffer to screen");
                            /* // this throws errors right now because deface() doesn't work.
                            for (_, c) in canvases.iter_mut() {
                                c.do_flushed();
                            }*/
                        }
                    }
                },
                _ => todo!("GAM: opcode not yet implemented"),
            }
        } else if let xous::Message::MutableBorrow(m) = &envelope.body {
            let mut buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let value = unsafe {
                archived_value_mut::<api::Opcode>(Pin::new(buf.as_mut()), m.id as usize)
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::RenderTextView(rtv) => {
                    let mut tv = rtv.deserialize(&mut xous_ipc::XousDeserializer {}).unwrap();
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
                                    let mut writer = rkyv::ser::serializers::BufferSerializer::new(buf);
                                    writer.serialize_value(&api::Opcode::RenderTextView(tv)).expect("GAM: couldn't re-archive return value");
                                    canvas.do_drawn().expect("GAM: couldn't set canvas to drawn");
                                } else {
                                    info!("GAM: attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored.");
                                }
                            } else {
                                info!("GAM: bogus GID {:?} in TextView {}, not doing anything in response to draw request.", tv.get_canvas_gid(), tv.text);
                                // silently fail if a bogus Gid is given???
                            }
                        },
                    };
                },
                rkyv::Archived::<api::Opcode>::SetCanvasBounds(rcb) => {
                    let mut cb: SetCanvasBoundsRequest = rcb.deserialize(&mut xous_ipc::XousDeserializer {}).unwrap();
                    if debug1{info!("GAM: SetCanvasBoundsRequest {:?}", cb);}
                    // ASSUME:
                    // very few canvases allow dynamic resizing, so we special case these
                    if cb.canvas == chatlayout.input {
                        let newheight = chatlayout.resize_input(cb.requested.y, &mut canvases).expect("GAM: SetCanvasBoundsRequest couldn't recompute input canvas height");
                        cb.granted = Some(newheight);
                        canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));

                        if ccc.connection.is_some() {
                            ccc.redraw_canvas().expect("GAM: couldn't issue redraw to content canvas");
                        }
                    } else {
                        cb.granted = None;
                    }
                    // pack our data back into the buffer to return
                    let mut writer = rkyv::ser::serializers::BufferSerializer::new(buf);
                    writer.serialize_value(&api::Opcode::SetCanvasBounds(cb)).expect("GAM: SetCanvasBoundsRequest couldn't re-archive return value");
                },
                rkyv::Archived::<api::Opcode>::RequestContentCanvas(rcc) => {
                    let mut req: ContentCanvasRequest = rcc.deserialize(&mut xous_ipc::XousDeserializer {}).unwrap();
                    if debug1{info!("GAM: RequestContentCanvas {:?}", req);}
                    // for now, we do nothing with the incoming gid value; but, in the future, we can use it
                    // as an authentication token perhaps to control access

                    //// here make a connection back to the requesting server, so that we can tell it to redraw if the layout has changed, etc.
                    if let Ok(cc) = xous_names::request_connection_blocking(req.servername.as_str().expect("GAM: malformed server name in content canvas request")) {
                        ccc.connection = Some(cc);
                    } else {
                        log::error!("GAM: content requestor gave us a bogus canvas result, aborting");
                        continue;
                    };

                    req.canvas = chatlayout.content;
                    // pack our data back into the buffer to return
                    let mut writer = rkyv::ser::serializers::BufferSerializer::new(buf);
                    writer.serialize_value(&api::Opcode::RequestContentCanvas(req)).expect("GAM: RequestContentCanvas couldn't re-archive return value");
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
                    let obj: GamObject = rtv.deserialize(&mut xous_ipc::XousDeserializer {}).unwrap();
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
                            canvas.do_drawn().expect("GAM: couldn't set canvas to drawn");
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
    }
}
