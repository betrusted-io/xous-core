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

use heapless::FnvIndexMap;
use heapless::consts::*;

use num_traits::FromPrimitive;
use xous_ipc::Buffer;
use api::Opcode;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};

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
    gfx: graphics_server::Gfx,
}
impl ChatLayout {
    pub fn init(xns: &xous_names::XousNames, trng: &trng::Trng, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ChatLayout, xous::Error> {
        let gfx = graphics_server::Gfx::new(&xns).unwrap();
        let screensize = gfx.screen_size().expect("Couldn't get screen size");
        // get the height of various text regions to compute the layout
        let small_height: i16 = gfx.glyph_height_hint(GlyphStyle::Small).expect("couldn't get glyph height") as i16;
        let regular_height: i16 = gfx.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height") as i16;
        let margin = 4;

        // allocate canvases in structures, and record their GID for future reference
        let status_canvas = Canvas::new(
            Rectangle::new_coords(0, 0, screensize.x, small_height),
            255, &trng, None
        ).expect("couldn't create status canvas");
        canvases.insert(status_canvas.gid(), status_canvas).expect("can't store status canvus");

        let predictive_canvas = Canvas::new(
            Rectangle::new_coords(0, screensize.y - regular_height - margin*2, screensize.x, screensize.y),
            254,
            &trng, None
        ).expect("couldn't create predictive text canvas");
        canvases.insert(predictive_canvas.gid(), predictive_canvas).expect("couldn't store predictive canvas");

        let min_input_height = regular_height + margin*2;
        let input_canvas = Canvas::new(
            Rectangle::new_v_stack(predictive_canvas.clip_rect(), -min_input_height),
            254, &trng, None
        ).expect("couldn't create input text canvas");
        canvases.insert(input_canvas.gid(), input_canvas).expect("couldn't store input canvas");

        let content_canvas = Canvas::new(
            Rectangle::new_v_span(status_canvas.clip_rect(), input_canvas.clip_rect()),
            128, &trng, None
        ).expect("couldn't create content canvas");
        canvases.insert(content_canvas.gid(), content_canvas).expect("can't store content canvas");

        Ok(ChatLayout {
            status: status_canvas.gid(),
            content: content_canvas.gid(),
            predictive: predictive_canvas.gid(),
            input: input_canvas.gid(),
            min_content_height: 64,
            min_input_height,
            gfx,
        })
    }
    pub fn clear(&self, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<(), xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("couldn't find input canvas");
        let content_canvas = canvases.get(&self.content).expect("couldn't find content canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("couldn't find predictive canvas");
        let status_canvas = canvases.get(&self.status).expect("couldn't find status canvas");

        let mut rect = status_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        self.gfx.draw_rectangle(rect).expect("can't clear canvas");

        let mut rect = content_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        self.gfx.draw_rectangle(rect).expect("can't clear canvas");

        let mut rect = predictive_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        self.gfx.draw_rectangle(rect).expect("can't clear canvas");

        let mut rect = input_canvas.clip_rect();
        rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
        self.gfx.draw_rectangle(rect).expect("can't clear canvas");
        Ok(())
    }
    pub fn resize_input(&mut self, new_height: i16, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<Point, xous::Error> {
        let input_canvas = canvases.get(&self.input).expect("couldn't find input canvas");
        let predictive_canvas = canvases.get(&self.predictive).expect("couldn't find predictive canvas");
        let status_canvas = canvases.get(&self.status).expect("couldn't find status canvas");

        let height: i16 = if new_height < self.min_input_height {
            self.min_input_height
        } else {
            new_height
        };
        let mut new_input_rect = Rectangle::new_v_stack(predictive_canvas.clip_rect(), -height);
        let mut new_content_rect = Rectangle::new_v_span(status_canvas.clip_rect(), new_input_rect);
        if (new_content_rect.br.y - new_content_rect.tl.y) > self.min_content_height {
            {
                let input_canvas_mut = canvases.get_mut(&self.input).expect("couldn't find input canvas");
                input_canvas_mut.set_clip(new_input_rect);
                new_input_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                self.gfx.draw_rectangle(new_input_rect).expect("can't clear canvas");
                    }
            {
                let content_canvas_mut = canvases.get_mut(&self.content).expect("couldn't find content canvas");
                content_canvas_mut.set_clip(new_content_rect);
                new_content_rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                self.gfx.draw_rectangle(new_content_rect).expect("can't clear canvas");
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
    fn init(trng: &trng::Trng, canvases: &mut FnvIndexMap<Gid, Canvas, U32>) -> Result<ModalCanvases, xous::Error> {
        let password_canvas = Canvas::new(
            Rectangle::new_coords(-1, -1, -1, -1),
            255, &trng, None
        ).expect("couldn't create password canvas");
        canvases.insert(password_canvas.gid(), password_canvas).expect("can't store password canvas");

        let menu_canvas = Canvas::new(
            Rectangle::new_coords(-1, -1, -1, -1),
            255, &trng, None
        ).expect("couldn't create menu canvas");
        canvases.insert(menu_canvas.gid(), menu_canvas).expect("can't store menu canvas");

        let alert_canvas = Canvas::new(
            Rectangle::new_coords(-1, -1, -1, -1),
            255, &trng, None
        ).expect("couldn't create alert canvas");
        canvases.insert(alert_canvas.gid(), alert_canvas).expect("can't store alert canvas");

        Ok(ModalCanvases {
            password: password_canvas.gid(),
            menu: menu_canvas.gid(),
            alert: alert_canvas.gid(),
        })
    }
}


#[xous::xous_main]
fn xmain() -> ! {
    let debugc = true;
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let gam_sid = xns.register_name(api::SERVER_NAME_GAM).expect("can't register server");
    log::trace!("starting up...");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let gfx = graphics_server::Gfx::new(&xns).expect("can't connect to GFX");
    let trng = trng::Trng::new(&xns).expect("can't connect to TRNG");

    let screensize = gfx.screen_size().expect("Couldn't get screen size");

    // a map of canvases accessable by Gid
    let mut canvases: FnvIndexMap<Gid, Canvas, U32> = FnvIndexMap::new();
    // let modallayout = ModalCanvases::init(trng, &mut canvases).expect("can't add modal layouts");
    let mut chatlayout = ChatLayout::init(&xns, &trng, &mut canvases).expect("couldn't create chat layout");
    chatlayout.clear(&mut canvases).expect("couldn't clear initial chatlayout");

    // now that all the initial canvases have been allocated, compute what canvases are drawable
    // this _replaces_ the original canvas structure, to avoid complications of tracking mutable references through compound data structures
    // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
    canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));

    // connect to the IME front end, and set its canvas
    info!("acquiring connection to IMEF...");
    let imef = ime_plugin_api::ImeFrontEnd::new(&xns).expect("Couldn't connect to IME front end");
    imef.set_input_canvas(chatlayout.input).expect("couldn't set IMEF input canvas");
    imef.set_prediction_canvas(chatlayout.predictive).expect("couldn't set IMEF prediction canvas");

    // ASSUME: shell is our default application, so set a default predictor of Shell
    log::trace!("acquiring connection to the default 'shell' predictor...");
    imef.set_predictor(xous::names::SERVER_NAME_IME_PLUGIN_SHELL).expect("couldn't set IMEF prediction to shell");
    // NOTE: all three API calls (set_input_canvas, set_prediction_canvas, set_predictor) are mandatory for IMEF initialization
    // now update the IMEF area, since we're initialized
    imef.redraw().unwrap();

    // no content canvas initially, but keep a placeholder for one
    let mut ccc: ContentCanvasConnection = ContentCanvasConnection{connection: None, redraw_id: None};

    if debugc{info!("chatlayout made st {:?} co {:?} pr {:?} in {:?}", chatlayout.status, chatlayout.content, chatlayout.predictive, chatlayout.input);}
    // make a thread to manage the status bar -- this needs to start after the IMEF is initialized
    // the status bar is a trusted element managed by the OS, and we are chosing to domicile this in the GAM process for now
    let status_gid = chatlayout.status.gid();
    log::trace!("starting status thread with gid {:?}", status_gid);
    xous::create_thread_4(status_thread, status_gid[0] as _, status_gid[1] as _, status_gid[2] as _, status_gid[3] as _).expect("couldn't create status thread");

    let mut powerdown_requested = false;
    let mut last_time: u64 = ticktimer.elapsed_ms();
    log::trace!("entering main loop");
    loop {
        let mut msg = xous::receive_message(gam_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::ClearCanvas) => {
                msg_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let gid = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.style = DrawStyle {fill_color: Some(PixelColor::Light), stroke_color: None, stroke_width: 0,};
                            gfx.draw_rectangle(rect).expect("can't clear canvas");
                        },
                        None => info!("attempt to clear bogus canvas, ignored."),
                    }
                });
            }
            Some(Opcode::GetCanvasBounds) => {
                msg_blocking_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let gid = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.normalize(); // normalize to 0,0 coordinates
                            xous::return_scalar2(msg.sender,
                                rect.tl.into(),
                                rect.br.into(),
                            ).expect("couldn't return canvas bounds");
                        },
                        None => info!("attempt to get bounds on bogus canvas gid {:?}, {:?} ignored.", gid, msg),
                    }
                })
            }
            Some(Opcode::PowerDownRequest) => {
                msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    powerdown_requested = true;
                    gfx.draw_sleepscreen().expect("couldn't draw sleep screen");
                    // a screen flush is part of the draw_sleepscreen abstraction
                    xous::return_scalar(msg.sender, 1).expect("couldn't confirm power down UI request");
                })
            }
            Some(Opcode::Redraw) => {
                msg_scalar_unpack!(msg, _, _, _, _, {
                    log::trace!("redraw message received");
                    if powerdown_requested {
                        continue; // don't allow any redraws if a powerdown is requested
                    }
                    let elapsed_time = ticktimer.elapsed_ms();
                    if elapsed_time - last_time > 33 {  // rate limit updates, no point in going faster than the eye can see
                        last_time = elapsed_time;

                        deface(&gfx, &mut canvases);
                        log::trace!("flushing...");
                        gfx.flush().expect("couldn't flush buffer to screen");
                        /* // this throws errors right now because deface() doesn't work.
                        for (_, c) in canvases.iter_mut() {
                            c.do_flushed();
                        }*/
                    }
                })
            }
            Some(Opcode::RenderTextView) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tv = buffer.to_original::<TextView, _>().unwrap();
                log::trace!("rendertextview {:?}", tv);
                match tv.get_op() {
                    TextOp::Nop => (),
                    TextOp::Render | TextOp::ComputeBounds => {
                        log::trace!("render request for {:?}", tv);
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
                                gfx.draw_textview(&mut tv_clone).expect("text view draw could not complete.");
                                // copy back the fields that we want to be mutable
                                log::trace!("got computed cursor of {:?}", tv_clone.cursor);
                                tv.cursor = tv_clone.cursor;
                                tv.bounds_computed = tv_clone.bounds_computed;

                                let ret = api::Return::RenderReturn(tv);
                                buffer.replace(ret).unwrap();
                                canvas.do_drawn().expect("couldn't set canvas to drawn");
                            } else {
                                info!("attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored.");
                            }
                        } else {
                            info!("bogus GID {:?} in TextView {}, not doing anything in response to draw request.", tv.get_canvas_gid(), tv.text);
                            // silently fail if a bogus Gid is given???
                        }
                    },
                };
            }
            Some(Opcode::SetCanvasBounds) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut cb = buffer.to_original::<SetCanvasBoundsRequest, _>().unwrap();
                log::trace!("SetCanvasBoundsRequest {:?}", cb);
                // ASSUME:
                // very few canvases allow dynamic resizing, so we special case these
                if cb.canvas == chatlayout.input {
                    let newheight = chatlayout.resize_input(cb.requested.y, &mut canvases).expect("SetCanvasBoundsRequest couldn't recompute input canvas height");
                    cb.granted = Some(newheight);
                    canvases = recompute_canvases(canvases, Rectangle::new(Point::new(0, 0), screensize));

                    if ccc.connection.is_some() {
                        ccc.redraw_canvas().expect("couldn't issue redraw to content canvas");
                    }
                } else {
                    cb.granted = None;
                }
                let ret = api::Return::SetCanvasBoundsReturn(cb);
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RequestContentCanvas) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut req = buffer.to_original::<ContentCanvasRequest, _>().unwrap();
                log::trace!("RequestContentCanvas {:?}", req);
                // for now, we do nothing with the incoming gid value; but, in the future, we can use it
                // as an authentication token perhaps to control access

                //// here make a connection back to the requesting server, so that we can tell it to redraw if the layout has changed, etc.
                if let Ok(cc) = xns.request_connection_blocking(req.servername.as_str().expect("malformed server name in content canvas request")) {
                    ccc.connection = Some(cc);
                    ccc.redraw_id = Some(req.redraw_scalar_id);
                } else {
                    log::error!("content requestor gave us a bogus canvas result, aborting");
                    continue;
                };

                req.canvas = chatlayout.content;
                let ret = api::Return::ContentCanvasReturn(req);
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RenderObject) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let obj = buffer.to_original::<GamObject, _>().unwrap();
                log::trace!("renderobject {:?}", obj);
                if let Some(canvas) = canvases.get_mut(&obj.canvas) {
                    // first, figure out if we should even be drawing to this canvas.
                    if canvas.is_drawable() {
                        match obj.obj {
                            GamObjectType::Line(mut line) => {
                                line.translate(canvas.clip_rect().tl);
                                line.translate(canvas.pan_offset());
                                gfx.draw_line_clipped(
                                    line,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw line");
                            },
                            GamObjectType::Circ(mut circ) => {
                                circ.translate(canvas.clip_rect().tl);
                                circ.translate(canvas.pan_offset());
                                gfx.draw_circle_clipped(
                                    circ,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw circle");
                            },
                            GamObjectType::Rect(mut rect) => {
                                rect.translate(canvas.clip_rect().tl);
                                rect.translate(canvas.pan_offset());
                                gfx.draw_rectangle_clipped(
                                    rect,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw rectangle");
                            },
                            GamObjectType::RoundRect(mut rr) => {
                                rr.translate(canvas.clip_rect().tl);
                                rr.translate(canvas.pan_offset());
                                gfx.draw_rounded_rectangle_clipped(
                                    rr,
                                    canvas.clip_rect(),
                                ).expect("couldn't draw rounded rectangle");
                            }
                        }
                        canvas.do_drawn().expect("couldn't set canvas to drawn");
                    } else {
                        info!("attempt to draw Object on non-drawable canvas. Not fatal, but request ignored.");
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
                log::trace!("leaving RenderObject");
            }
            None => {log::error!("unhandled message {:?}", msg); break}
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(gam_sid).unwrap();
    xous::destroy_server(gam_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
