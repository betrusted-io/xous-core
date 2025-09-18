mod api;
use api::*;
mod canvas;
use canvas::*;
mod tokens;
use ime_plugin_api::ApiToken;
use tokens::*;
mod layouts;
use layouts::*;
mod contexts;
use contexts::*;
mod bip39;

use core::sync::atomic::{AtomicU32, Ordering};
use std::collections::HashMap;

use api::Opcode;
#[cfg(feature = "bao1x")]
use bao1x_hal_service::trng;
use blitstr2::GlyphStyle;
use gam::{MAIN_MENU_NAME, ROOTKEY_MODAL_NAME};
use log::info;
use num_traits::*;
use ux_api::minigfx::*;
use ux_api::service::api::*;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};
use xous_ipc::Buffer;

/// This sets the initial app focus on boot
const INITIAL_APP_FOCUS: &'static str = gam::APP_NAME_SHELLCHAT;

static CB_TO_MAIN_CONN: AtomicU32 = AtomicU32::new(0);
fn imef_cb(s: String) {
    if CB_TO_MAIN_CONN.load(Ordering::Relaxed) != 0 {
        let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
        let buf = xous_ipc::Buffer::into_buf(s).or(Err(xous::Error::InternalError)).unwrap();
        buf.lend(cb_to_main_conn, Opcode::InputLine.to_u32().unwrap()).unwrap();
    }
}
fn main() -> ! {
    #[cfg(not(feature = "ditherpunk"))]
    wrapped_main();

    #[cfg(feature = "ditherpunk")]
    let stack_size = 1024 * 1024;
    #[cfg(feature = "ditherpunk")]
    std::thread::Builder::new().stack_size(stack_size).spawn(wrapped_main).unwrap().join().unwrap()
}
fn wrapped_main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed; this is a gateway server
    let gam_sid = xns.register_name(api::SERVER_NAME_GAM, None).expect("can't register server");
    CB_TO_MAIN_CONN.store(xous::connect(gam_sid).unwrap(), Ordering::Relaxed);

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let gfx = graphics_server::Gfx::new(&xns).expect("can't connect to GFX");
    let trng = trng::Trng::new(&xns).expect("can't connect to TRNG");

    let mut context_mgr = ContextManager::new(&xns);

    // a map of canvases accessable by Gid
    let mut canvases: HashMap<Gid, Canvas> = HashMap::new();

    let screensize = gfx.screen_size().expect("Couldn't get screen size");
    // the status canvas is special -- there can only be one, and it is ultimately trusted
    #[cfg(feature = "bao1x")]
    let glyph_height_hint = gfx.glyph_height_hint(GlyphStyle::Tall).expect("couldn't get glyph height");
    #[cfg(not(feature = "bao1x"))]
    let glyph_height_hint = gfx.glyph_height_hint(GlyphStyle::Cjk).expect("couldn't get glyph height");
    let status_canvas = Canvas::new(
        Rectangle::new_coords(
            0,
            0,
            screensize.x,
            // note: if this gets modified, the "pop" routine in gfx/backend/betrusted.rs also needs to be
            // updated
            glyph_height_hint as isize * 2,
        ),
        255,
        &trng,
        None,
        crate::api::CanvasType::Status,
    )
    .expect("couldn't create status canvas");
    let status_cliprect = status_canvas.clip_rect();
    status_canvas.set_onscreen(true);
    status_canvas.set_drawable(true);
    let status_gid = status_canvas.gid().gid();
    canvases.insert(status_canvas.gid(), status_canvas);
    recompute_canvases(&canvases);

    // initialize the status bar -- this needs to start late, after the IMEF and most other things are
    // initialized this used to be domiciled in the GAM, but we split it out because this started to pull
    // too much functionality into the GAM and was causing circular crate conflicts with sub-functions
    // that the status bar relies upon. we do a hack to try and push a GID to the status bar "securely":
    // we introduce a race condition where we hope that the GAM is the first thing to talk to the status
    // bar, and the first message is its GID to render on. generally should be OK, because during boot,
    // all processes are trusted...
    let status_conn = xns
        .request_connection_blocking("_Status bar GID receiver_")
        .expect("couldn't connect to status bar GID receiver");
    xous::send_message(
        status_conn,
        xous::Message::new_scalar(
            0, // message type doesn't matter because there is only one message it should ever receive
            status_gid[0] as usize,
            status_gid[1] as usize,
            status_gid[2] as usize,
            status_gid[3] as usize,
        ),
    )
    .expect("couldn't set status GID");

    // a random number we can use to identify ourselves between API calls
    let gam_token =
        [trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap(), trng.get_u32().unwrap()];

    let mut powerdown_requested = false;
    let mut last_time: u64 = ticktimer.elapsed_ms();
    let mut did_test = false; // allow one go at the test pattern
    log::trace!("entering main loop");

    #[cfg(not(target_os = "xous"))]
    {
        log::info!("********************************************************************************");
        log::info!("USAGE:");
        log::info!("   `Home` key to bring up menu; arrow keys to go up/down; `Home` to select again");
        log::info!("   `F1`-`F4` to pick predictions; `F5` and `F6` generate test unicode characters");
        log::info!("   Otherwise type in the GUI window; `help` for the current command list");
        log::info!("   ^C in the console window (this window) to quit");
        log::info!("********************************************************************************");
    }
    loop {
        let mut msg = xous::receive_message(gam_sid).unwrap();
        let op = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", op);
        match op {
            Some(Opcode::ClearCanvas) => {
                msg_scalar_unpack!(msg, g0, g1, g2, g3, {
                    let gid = Gid::new([g0 as _, g1 as _, g2 as _, g3 as _]);
                    match canvases.get(&gid) {
                        Some(c) => {
                            let mut rect = c.clip_rect();
                            rect.style = DrawStyle {
                                fill_color: Some(PixelColor::Light),
                                stroke_color: None,
                                stroke_width: 0,
                            };
                            gfx.draw_rectangle(rect).expect("can't clear canvas");
                        }
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
                            log::trace!("getcanvasbounds: {:?}", rect);
                            xous::return_scalar2(msg.sender, rect.tl.into(), rect.br.into())
                                .expect("couldn't return canvas bounds");
                        }
                        None => {
                            info!("attempt to get bounds on bogus canvas gid {:?}, {:?} ignored.", gid, msg)
                        }
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
            Some(Opcode::ShipModeBlankRequest) => {
                msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    powerdown_requested = true;
                    gfx.draw_rectangle(Rectangle::new_with_style(
                        Point::new(0, 0),
                        screensize,
                        DrawStyle::new(PixelColor::Light, PixelColor::Light, 0),
                    ))
                    .expect("couldn't clear screen");
                    gfx.flush().expect("couldn't refresh blank screen");
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
                    if elapsed_time - last_time > gam::RATE_LIMIT_MS as u64 {
                        // rate limit updates, no point in going faster than the eye can see
                        last_time = elapsed_time;

                        if deface(&gfx, &trng, &mut canvases) {
                            // we keep this here because it's a fail-safe in case prior routines missed an
                            // edge case. shoot out a warning noting the issue.
                            log::warn!(
                                "canvases were not defaced in order. running a defacement, but this could result in drawing optimizations failing."
                            );
                            // try to redraw the trusted foreground apps after a defacement
                            log::trace!("deface redraw");
                            context_mgr.redraw().expect("couldn't redraw after defacement");
                        }
                        log::trace!("flushing...");
                        gfx.flush().expect("couldn't flush buffer to screen");

                        for (_, c) in canvases.iter_mut() {
                            c.do_flushed().expect("couldn't update flushed state");
                        }
                    }
                })
            }
            Some(Opcode::SetDebugLevel) => msg_blocking_scalar_unpack!(msg, level, _, _, _, {
                match level {
                    0 => log::set_max_level(log::LevelFilter::Info),
                    1 => log::set_max_level(log::LevelFilter::Debug),
                    2 => log::set_max_level(log::LevelFilter::Trace),
                    _ => log::set_max_level(log::LevelFilter::Info),
                }
                xous::return_scalar(msg.sender, level).unwrap();
            }),
            Some(Opcode::RenderTextView) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tv = buffer.to_original::<TextView, _>().unwrap();
                log::trace!("rendertextview {:?}", tv);
                match tv.get_op() {
                    TextOp::Nop => (),
                    TextOp::Render => {
                        if tv.invert & tv.token.is_some() {
                            // an inverted text can only be made by secure processes. check that it has a
                            // valid token.
                            if !context_mgr.is_token_valid(tv.token.unwrap()) {
                                log::error!(
                                    "Attempt to draw inverted text without valid credentials. Aborting."
                                );
                                continue;
                            }
                        }

                        log::trace!("render request for {:?}", tv);

                        if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                            tv.set_dry_run(!canvas.is_onscreen());
                            // if we're requesting inverted text, this better be a "trusted canvas"
                            // BOOT_CONTEXT_TRUSTLEVEL is reserved for the "status bar"
                            // BOOT_CONTEXT_TRUSTLEVEL - 1 is where e.g. password modal dialog boxes end up
                            if tv.invert & (canvas.trust_level() < BOOT_CONTEXT_TRUSTLEVEL - 1) {
                                log::error!(
                                    "Attempt to draw inverted text without sufficient trust level: {}. Aborting.",
                                    canvas.trust_level()
                                );
                                continue;
                            }
                            // first, figure out if we should even be drawing to this canvas.
                            if canvas.is_drawable() {
                                // set the clip rectangle according to the canvas' location
                                let base_clip_rect = canvas.clip_rect();
                                tv.clip_rect = Some(base_clip_rect.into());

                                // you have to clone the tv object, because if you don't the same block of
                                // memory gets passed on to the graphics_server(). Which is efficient, but,
                                // the call will automatically Drop() the memory, which causes a panic when
                                // this routine returns.
                                let mut tv_clone = tv.clone();
                                // issue the draw command
                                gfx.draw_textview(&mut tv_clone).expect("text view draw could not complete.");
                                // copy back the fields that we want to be mutable
                                tv.cursor = tv_clone.cursor;
                                tv.bounds_computed = tv_clone.bounds_computed;
                                tv.overflow = tv_clone.overflow;
                                tv.busy_animation_state = tv_clone.busy_animation_state;

                                let ret = api::Return::RenderReturn(tv);
                                buffer.replace(ret).unwrap();
                                if canvas.is_onscreen() {
                                    canvas.do_drawn().expect("couldn't set canvas to drawn");
                                }
                            } else {
                                log::debug!(
                                    "attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored. {:?}",
                                    tv
                                );
                                let ret = api::Return::NotCurrentlyDrawable;
                                buffer.replace(ret).unwrap();
                            }
                        } else {
                            info!(
                                "bogus GID {:?} in TextView {}, not doing anything in response to draw request.",
                                tv.get_canvas_gid(),
                                tv.text
                            );
                            // silently fail if a bogus Gid is given???
                        }
                    }
                    TextOp::ComputeBounds => {
                        log::trace!("bounds request for {:?}", tv);
                        tv.set_dry_run(true);

                        if tv.clip_rect.is_none() {
                            // fill in the clip rect from the canvas
                            if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                                // set the clip rectangle according to the canvas' location
                                let mut base_clip_rect = canvas.clip_rect();
                                base_clip_rect.normalize();
                                tv.clip_rect = Some(base_clip_rect.into());
                            } else {
                                info!(
                                    "bogus GID {:?} in TextView {}, not doing anything in response to draw request.",
                                    tv.get_canvas_gid(),
                                    tv.text
                                );
                                // silently fail if a bogus Gid is given???
                                continue;
                            }
                        }
                        let mut tv_clone = tv.clone();
                        // issue the draw command
                        gfx.draw_textview(&mut tv_clone).expect("text view draw could not complete.");
                        // copy back the fields that we want to be mutable
                        log::trace!(
                            "got computed cursor of {:?}, bounds {:?}",
                            tv_clone.cursor,
                            tv_clone.bounds_computed
                        );
                        tv.cursor = tv_clone.cursor;
                        tv.bounds_computed = tv_clone.bounds_computed;
                        tv.overflow = tv_clone.overflow;
                        tv.busy_animation_state = tv_clone.busy_animation_state;

                        let ret = api::Return::RenderReturn(tv);
                        buffer.replace(ret).unwrap();
                    }
                };
            }
            Some(Opcode::SetCanvasBounds) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut cb = buffer.to_original::<SetCanvasBoundsRequest, _>().unwrap();
                log::trace!("SetCanvasBoundsRequest {:?}", cb);

                let granted = if cb.token_type == TokenType::Gam {
                    context_mgr.set_canvas_height(
                        &gfx,
                        cb.token,
                        cb.requested.y,
                        &status_cliprect,
                        &mut canvases,
                    )
                } else {
                    context_mgr.set_canvas_height_app_token(
                        &gfx,
                        cb.token,
                        cb.requested.y,
                        &status_cliprect,
                        &mut canvases,
                    )
                };
                if granted.is_some() {
                    // recompute the canvas orders based on the new layout
                    recompute_canvases(&canvases);
                    // this set of redraw commands is not needed because every context will call redraw after
                    // it has finished fitting its bounds log::info!("canvas bounds
                    // redraw"); context_mgr.redraw().expect("can't redraw after new
                    // canvas bounds");
                }
                cb.granted = granted;
                let ret = api::Return::SetCanvasBoundsReturn(cb);
                log::trace!("returning {:?}", cb);
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RequestContentCanvas) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let req = buffer.to_original::<[u32; 4], _>().unwrap();
                log::trace!("RequestContentCanvas {:?}", req);

                let ret = api::Return::ContentCanvasReturn(context_mgr.get_content_canvas(req));
                buffer.replace(ret).unwrap();
            }
            #[cfg(feature = "ditherpunk")]
            Some(Opcode::RenderTile) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut obj = buffer.to_original::<GamTile, _>().unwrap();
                log::debug!("RenderTile {:?}", obj);
                if let Some(canvas) = canvases.get_mut(&obj.canvas) {
                    // first, figure out if we should even be drawing to this canvas.
                    log::debug!(
                        "drawable {} onscreen {} state{:?} for canvas {:?}",
                        canvas.is_drawable(),
                        canvas.is_onscreen(),
                        canvas.state(),
                        canvas.gid()
                    );
                    if canvas.is_drawable() && canvas.is_onscreen() {
                        obj.tile.translate(canvas.clip_rect().tl);
                        obj.tile.translate(canvas.pan_offset());
                        log::trace!("drawing tile {:?}", obj.tile);
                        gfx.draw_tile_clipped(obj.tile, canvas.clip_rect()).expect("couldn't draw bitmap");
                        canvas.do_drawn().expect("couldn't set canvas to drawn");
                    } else {
                        log::info!(
                            "attempt to draw Object on non-drawable canvas. Not fatal, but request ignored: {:?}",
                            obj
                        );
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
                log::trace!("leaving RenderTile");
            }
            Some(Opcode::RenderObject) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let obj = buffer.to_original::<GamObject, _>().unwrap();
                log::trace!("renderobject {:?}", obj);
                if let Some(canvas) = canvases.get_mut(&obj.canvas) {
                    // first, figure out if we should even be drawing to this canvas.
                    log::debug!(
                        "drawable {} onscreen {} state{:?} for canvas {:?}",
                        canvas.is_drawable(),
                        canvas.is_onscreen(),
                        canvas.state(),
                        canvas.gid()
                    );
                    if canvas.is_drawable() && canvas.is_onscreen() {
                        match obj.obj {
                            GamObjectType::Line(mut line) => {
                                line.translate(canvas.clip_rect().tl);
                                line.translate(canvas.pan_offset());
                                gfx.draw_line_clipped(line, canvas.clip_rect()).expect("couldn't draw line");
                            }
                            GamObjectType::Circ(mut circ) => {
                                circ.translate(canvas.clip_rect().tl);
                                circ.translate(canvas.pan_offset());
                                gfx.draw_circle_clipped(circ, canvas.clip_rect())
                                    .expect("couldn't draw circle");
                            }
                            GamObjectType::Rect(mut rect) => {
                                rect.translate(canvas.clip_rect().tl);
                                rect.translate(canvas.pan_offset());
                                gfx.draw_rectangle_clipped(rect, canvas.clip_rect())
                                    .expect("couldn't draw rectangle");
                            }
                            GamObjectType::RoundRect(mut rr) => {
                                rr.translate(canvas.clip_rect().tl);
                                rr.translate(canvas.pan_offset());
                                gfx.draw_rounded_rectangle_clipped(rr, canvas.clip_rect())
                                    .expect("couldn't draw rounded rectangle");
                            }
                        }
                        canvas.do_drawn().expect("couldn't set canvas to drawn");
                    } else {
                        log::debug!(
                            "attempt to draw Object on non-drawable canvas. Not fatal, but request ignored: {:?}",
                            obj
                        );
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
                log::trace!("leaving RenderObject");
            }
            Some(Opcode::RenderObjectList) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let obj_ipc = buffer.to_original::<GamObjectList, _>().unwrap();
                if let Some(canvas) = canvases.get_mut(&obj_ipc.canvas) {
                    // first, figure out if we should even be drawing to this canvas.
                    if canvas.is_drawable() && canvas.is_onscreen() {
                        let mut obj_list = ClipObjectList::default();
                        for item in obj_ipc.list.iter() {
                            if let Some(obj) = item {
                                match obj {
                                    GamObjectType::Line(mut line) => {
                                        line.translate(canvas.clip_rect().tl);
                                        line.translate(canvas.pan_offset());
                                        obj_list
                                            .push(ClipObjectType::Line(line), canvas.clip_rect())
                                            .unwrap();
                                    }
                                    GamObjectType::Circ(mut circ) => {
                                        circ.translate(canvas.clip_rect().tl);
                                        circ.translate(canvas.pan_offset());
                                        obj_list
                                            .push(ClipObjectType::Circ(circ), canvas.clip_rect())
                                            .unwrap();
                                    }
                                    GamObjectType::Rect(mut rect) => {
                                        rect.translate(canvas.clip_rect().tl);
                                        rect.translate(canvas.pan_offset());
                                        obj_list
                                            .push(ClipObjectType::Rect(rect), canvas.clip_rect())
                                            .unwrap();
                                    }
                                    GamObjectType::RoundRect(mut rr) => {
                                        rr.translate(canvas.clip_rect().tl);
                                        rr.translate(canvas.pan_offset());
                                        obj_list
                                            .push(ClipObjectType::RoundRect(rr), canvas.clip_rect())
                                            .unwrap();
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        gfx.draw_object_list_clipped(obj_list).expect("couldn't draw object list");
                        canvas.do_drawn().expect("couldn't set canvas to drawn");
                    } else {
                        log::debug!(
                            "attempt to draw Object on non-drawable canvas. Not fatal, but request ignored: {:?}",
                            obj_ipc
                        );
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
            }
            Some(Opcode::ClaimToken) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tokenclaim = buffer.to_original::<TokenClaim, _>().unwrap();
                tokenclaim.token = context_mgr.claim_token(tokenclaim.name.as_str());
                buffer.replace(tokenclaim).unwrap();
            }
            Some(Opcode::PredictorApiToken) => {
                let buf = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let at = buf.to_original::<ApiToken, _>().unwrap();
                context_mgr.set_pred_api_token(at);
            }
            Some(Opcode::TrustedInitDone) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if context_mgr.allow_untrusted_code() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::RegisterUx) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let registration = buffer.to_original::<UxRegistration, _>().unwrap();

                let init_focus_found =
                    if registration.app_name.as_str() == INITIAL_APP_FOCUS { true } else { false };
                // note that we are currently assigning all Ux registrations a trust level consistent with a
                // boot context (ultimately trusted) this needs to be modified later on once
                // we allow post-boot apps to be created
                let token = context_mgr.register(&gfx, &trng, &status_cliprect, &mut canvases, registration);

                // compute what canvases are drawable
                // this _replaces_ the original canvas structure, to avoid complications of tracking mutable
                // references through compound data structures this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
                recompute_canvases(&canvases);

                buffer.replace(Return::UxToken(token)).unwrap();

                // fire off a thread that deals with activating the initial boot context. You need this
                // because this call has to complete before the context can respond to activation events.
                if token.is_some() & init_focus_found {
                    std::thread::spawn({
                        let gam_token = gam_token.clone();
                        let conn = CB_TO_MAIN_CONN.load(Ordering::SeqCst);
                        move || {
                            let switchapp =
                                SwitchToApp { token: gam_token, app_name: String::from(INITIAL_APP_FOCUS) };
                            let buf = Buffer::into_buf(switchapp).or(Err(xous::Error::InternalError))?;
                            buf.send(conn, Opcode::SwitchToApp.to_u32().unwrap())
                                .or(Err(xous::Error::InternalError))
                                .map(|_| ())
                        }
                    });
                }
            }
            Some(Opcode::SetAudioOpcode) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let audio_op = buffer.to_original::<SetAudioOpcode, _>().unwrap();
                context_mgr.set_audio_op(audio_op);
            }
            Some(Opcode::InputLine) => {
                // receive the keyboard input and pass it on to the context with focus
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let inputline = buffer.to_original::<String, _>().unwrap();
                log::debug!("received input line, forwarding on... {}", inputline);
                match context_mgr.forward_input(inputline) {
                    Err(e) => log::warn!("InputLine missed its target {:?}; input ignored", e),
                    _ => (),
                }
                log::debug!("returned from forward_input");
            }
            Some(Opcode::KeyboardEvent) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    core::char::from_u32(k1 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k2 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k3 as u32).unwrap_or('\u{0000}'),
                    core::char::from_u32(k4 as u32).unwrap_or('\u{0000}'),
                ];
                context_mgr.key_event(keys, &gfx, &mut canvases);
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena != 0 { context_mgr.vibe(true) } else { context_mgr.vibe(false) }
            }),
            Some(Opcode::ToggleMenuMode) => msg_scalar_unpack!(msg, t1, t2, t3, t4, {
                let token = [t1 as u32, t2 as u32, t3 as u32, t4 as u32];
                context_mgr.toggle_menu_mode(token);
            }),
            Some(Opcode::RevertFocus) => match context_mgr.revert_focus(&gfx, &mut canvases) {
                Ok(_) => xous::return_scalar(msg.sender, 0).expect("couldn't unblock caller"),
                _ => xous::return_scalar(msg.sender, 1).expect("couldn't unblock caller"),
            },
            Some(Opcode::RevertFocusNb) => match context_mgr.revert_focus(&gfx, &mut canvases) {
                Ok(_) => {}
                Err(e) => log::warn!("failed to revert focus: {:?}", e),
            },
            Some(Opcode::QueryGlyphProps) => msg_blocking_scalar_unpack!(msg, style, _, _, _, {
                let height = gfx
                    .glyph_height_hint(GlyphStyle::from(style))
                    .expect("couldn't query glyph height from gfx");
                xous::return_scalar(msg.sender, height).expect("could not return QueryGlyphProps request");
            }),
            Some(Opcode::RedrawIme) => {
                context_mgr.redraw_imef().expect("couldn't redraw the IMEF");
            }
            Some(Opcode::SwitchToApp) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let switchapp = buffer.to_original::<SwitchToApp, _>().unwrap();
                log::debug!(
                    "trying to switch to {:?} with token {:?}",
                    switchapp.app_name.as_str(),
                    switchapp.token
                );

                if let Some(new_app_token) = context_mgr.find_app_token_by_name(switchapp.app_name.as_str()) {
                    if new_app_token != context_mgr.focused_app().unwrap_or([0, 0, 0, 0]) {
                        // two things:
                        // 1. [0, 0, 0, 0] is simply a very unlikely GID because it's a 128 bit TRNG, and this
                        //    allows us to handle
                        // the power-on case where we have no focused app cleanly.
                        // 2. the `activate()` method on `context_mgr` does not handle app-to-app switches
                        //    when you're the same app.
                        // this is because there is a rule which says if you're going to put an app on top of
                        // an app, take the old app and hide it, while also taking the
                        // new app and showing it. This is a race condition, and it results
                        // in the "same-app" switch ending up in a hidden state, because within the routine
                        // the entering state is set before the leaving state due to
                        // interior-mutability issues (see the block under the debug statement
                        // titled "resolving visibility rules"). The fix is to simply not issue a context
                        // switch if it's the same app. It saves CPU thrashing and
                        // also works around this problem. (see issue #145)
                        #[cfg(not(feature = "unsafe-app-loading"))]
                        let authorized_switchers =
                            vec![MAIN_MENU_NAME, ROOTKEY_MODAL_NAME, gam::STATUS_BAR_NAME];
                        #[cfg(feature = "unsafe-app-loading")]
                        let authorized_switchers = [
                            &[MAIN_MENU_NAME, ROOTKEY_MODAL_NAME, gam::STATUS_BAR_NAME],
                            gam::EXPECTED_APP_CONTEXTS,
                        ]
                        .concat()
                        .to_vec();
                        for switchers in authorized_switchers {
                            if let Some(auth_token) = context_mgr.find_app_token_by_name(switchers) {
                                if auth_token == switchapp.token {
                                    match context_mgr.activate(&gfx, &mut canvases, new_app_token, false) {
                                        Ok(_) => (),
                                        Err(_) => log::warn!(
                                            "failed to switch to {}, silent error!",
                                            switchapp.app_name.as_str()
                                        ),
                                    }
                                    continue;
                                }
                            }
                        }
                        // this message came from ourselves
                        if gam_token == switchapp.token {
                            match context_mgr.activate(&gfx, &mut canvases, new_app_token, true) {
                                Ok(_) => (),
                                Err(_) => log::warn!(
                                    "failed to switch to {}, silent error!",
                                    switchapp.app_name.as_str()
                                ),
                            }
                        }
                    }
                }
            }
            Some(Opcode::RaiseMenu) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut activation = buffer.to_original::<GamActivation, _>().unwrap();
                log::debug!("got request to raise context {}", activation.name);
                let result = context_mgr.raise_menu(activation.name.as_str(), &gfx, &mut canvases);
                activation.result = Some(match result {
                    Ok(_) => ActivationResult::Success,
                    Err(_) => ActivationResult::Failure,
                });
                buffer.replace(activation).unwrap();
            }
            Some(Opcode::Devboot) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena != 0 {
                    gfx.set_devboot(true).expect("couldn't send devboot message");
                } else {
                    gfx.set_devboot(false).expect("couldn't send devboot message");
                }
            }),
            Some(Opcode::TestPattern) => msg_blocking_scalar_unpack!(msg, duration_ms, _, _, _, {
                if !did_test {
                    did_test = true;
                    let checked_duration = if duration_ms > 60_000 { 60_000 } else { duration_ms };
                    gfx.selftest(checked_duration);
                }
                xous::return_scalar(msg.sender, 1).expect("couldn't ack self test");
            }),
            Some(Opcode::Bip39toBytes) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut spec = buffer.to_original::<Bip39Ipc, _>().unwrap();
                let mut phrase = Vec::<std::string::String>::new();
                for maybe_word in spec.words.iter() {
                    if let Some(word) = maybe_word {
                        phrase.push(word.as_str().to_string());
                    }
                }
                match bip39::bip39_to_bytes(&phrase) {
                    Ok(data) => {
                        spec.data_len = data.len() as u32;
                        spec.data[..data.len()].copy_from_slice(&data);
                    }
                    Err(_) => {
                        // zero-length data indicates an error
                        spec.data_len = 0;
                    }
                }
                buffer.replace(spec).unwrap();
            }
            Some(Opcode::BytestoBip39) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut spec = buffer.to_original::<Bip39Ipc, _>().unwrap();
                let data = spec.data[..spec.data_len as usize].to_vec();
                match bip39::bytes_to_bip39(&data) {
                    Ok(phrase) => {
                        for (word, returned) in phrase.iter().zip(spec.words.iter_mut()) {
                            *returned = Some(String::from(word));
                        }
                    }
                    Err(_) => {
                        for returned in spec.words.iter_mut() {
                            *returned = None;
                        }
                    }
                }
                buffer.replace(spec).unwrap();
            }
            Some(Opcode::Bip39Suggestions) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut spec = buffer.to_original::<Bip39Ipc, _>().unwrap();
                let start = std::str::from_utf8(&spec.data[..spec.data_len as usize]).unwrap_or("");
                let suggestions = bip39::suggest_bip39(start);
                for (word, returned) in suggestions.iter().zip(spec.words.iter_mut()) {
                    *returned = Some(String::from(word));
                }
                buffer.replace(spec).unwrap();
            }
            Some(Opcode::AllowMainMenu) => {
                context_mgr.allow_mainmenu();
                xous::return_scalar(msg.sender, 0).ok();
            }
            #[cfg(feature = "unsafe-app-loading")]
            Some(Opcode::RegisterName) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let registration = buffer.to_original::<NameRegistration, _>().unwrap();
                gfx.set_devboot(true).ok(); // indicate to users that we are no longer in a codebase that is exclusively trusted code
                context_mgr.register_name(registration.name.to_str(), &registration.auth_token);
            }
            Some(Opcode::Quit) => break,
            None => {
                log::error!("unhandled message {:?}", msg);
            }
        }

        // we don't currently have a mechanism to guarantee that the default app has focus
        // right now, it just depends upon it requesting focus, and none others taking it.
        // probably something should be inserted around here to take care of that?
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(gam_sid).unwrap();
    xous::destroy_server(gam_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
