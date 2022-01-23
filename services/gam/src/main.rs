#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

mod api;
use api::*;
mod canvas;
use canvas::*;
mod tokens;
use tokens::*;
mod layouts;
use layouts::*;
mod contexts;
use contexts::*;

use graphics_server::*;
use xous_ipc::{Buffer, String};
use api::Opcode;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};
use gam::{MAIN_MENU_NAME, ROOTKEY_MODAL_NAME};

use log::info;
use std::collections::HashMap;
use num_traits::*;
use core::{sync::atomic::{AtomicU32, Ordering}};

static CB_TO_MAIN_CONN: AtomicU32 = AtomicU32::new(0);
fn imef_cb(s: String::<4000>) {
    if CB_TO_MAIN_CONN.load(Ordering::Relaxed) != 0 {
        let cb_to_main_conn = CB_TO_MAIN_CONN.load(Ordering::Relaxed);
        let buf = xous_ipc::Buffer::into_buf(s).or(Err(xous::Error::InternalError)).unwrap();
        buf.lend(cb_to_main_conn, Opcode::InputLine.to_u32().unwrap()).unwrap();
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed; this is a gateway server
    let gam_sid = xns.register_name(api::SERVER_NAME_GAM, None).expect("can't register server");
    CB_TO_MAIN_CONN.store(xous::connect(gam_sid).unwrap(), Ordering::Relaxed);
    log::trace!("starting up...");

    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

    let gfx = graphics_server::Gfx::new(&xns).expect("can't connect to GFX");
    let trng = trng::Trng::new(&xns).expect("can't connect to TRNG");

    let mut context_mgr = ContextManager::new(&xns);

    // a map of canvases accessable by Gid
    let mut canvases: HashMap<Gid, Canvas> = HashMap::new();

    let screensize = gfx.screen_size().expect("Couldn't get screen size");
    // the status canvas is special -- there can only be one, and it is ultimately trusted
    let status_canvas = Canvas::new(
        Rectangle::new_coords(0, 0, screensize.x, gfx.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height") as i16 * 2),
        255, &trng, None, crate::api::CanvasType::Status
    ).expect("couldn't create status canvas");
    canvases.insert(status_canvas.gid(), status_canvas);
    canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));

    // initialize the status bar -- this needs to start late, after the IMEF and most other things are initialized
    // this used to be domiciled in the GAM, but we split it out because this started to pull too much functionality
    // into the GAM and was causing circular crate conflicts with sub-functions that the status bar relies upon.
    // we do a hack to try and push a GID to the status bar "securely": we introduce a race condition where we hope
    // that the GAM is the first thing to talk to the status bar, and the first message is its GID to render on.
    // generally should be OK, because during boot, all processes are trusted...
    let status_gid = status_canvas.gid().gid();
    log::trace!("initializing status bar with gid {:?}", status_gid);
    let status_conn = xns.request_connection_blocking("_Status bar GID receiver_").expect("couldn't connect to status bar GID receiver");
    xous::send_message(status_conn,
        xous::Message::new_scalar(0, // message type doesn't matter because there is only one message it should ever receive
        status_gid[0] as usize, status_gid[1] as usize, status_gid[2] as usize, status_gid[3] as usize
        )
    ).expect("couldn't set status GID");

    let mut powerdown_requested = false;
    let mut last_time: u64 = ticktimer.elapsed_ms();
    let mut did_test = false; // allow one go at the test pattern
    log::trace!("entering main loop");

    #[cfg(not(any(target_os = "none", target_os = "xous")))]
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
                            log::trace!("getcanvasbounds: {:?}", rect);
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
            Some(Opcode::ShipModeBlankRequest) => {
                msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    powerdown_requested = true;
                    gfx.draw_rectangle(Rectangle::new_with_style(Point::new(0,0), screensize, DrawStyle::new(PixelColor::Light, PixelColor::Light, 0))).expect("couldn't clear screen");
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
                    if elapsed_time - last_time > 33 {  // rate limit updates, no point in going faster than the eye can see
                        last_time = elapsed_time;

                        if deface(&gfx, &trng, &mut canvases) {
                            // we keep this here because it's a fail-safe in case prior routines missed an edge case. shoot out a warning noting the issue.
                            log::warn!("canvases were not defaced in order. running a defacement, but this could result in drawing optimizations failing.");
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
            Some(Opcode::RenderTextView) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tv = buffer.to_original::<TextView, _>().unwrap();
                log::trace!("rendertextview {:?}", tv);
                match tv.get_op() {
                    TextOp::Nop => (),
                    TextOp::Render => {
                        if tv.invert & tv.token.is_some() {
                            // an inverted text can only be made by secure processes. check that it has a valid token.
                            if !context_mgr.is_token_valid(tv.token.unwrap()) {
                                log::error!("Attempt to draw inverted text without valid credentials. Aborting.");
                                continue;
                            }
                        }

                        log::trace!("render request for {:?}", tv);
                        tv.set_dry_run(false);

                        if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                            // if we're requesting inverted text, this better be a "trusted canvas"
                            // BOOT_CONTEXT_TRUSTLEVEL is reserved for the "status bar"
                            // BOOT_CONTEXT_TRUSTLEVEL - 1 is where e.g. password modal dialog boxes end up
                            if tv.invert & (canvas.trust_level() < BOOT_CONTEXT_TRUSTLEVEL - 1) {
                                log::error!("Attempt to draw inverted text without sufficient trust level: {}. Aborting.", canvas.trust_level());
                                continue;
                            }
                            // first, figure out if we should even be drawing to this canvas.
                            if canvas.is_drawable() { // dry runs should not move any pixels so they are OK to go through in any case
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

                                let ret = api::Return::RenderReturn(tv);
                                buffer.replace(ret).unwrap();
                                canvas.do_drawn().expect("couldn't set canvas to drawn");
                            } else {
                                log::debug!("attempt to draw TextView on non-drawable canvas. Not fatal, but request ignored. {:?}", tv);
                                let ret = api::Return::NotCurrentlyDrawable;
                                buffer.replace(ret).unwrap();
                            }
                        } else {
                            info!("bogus GID {:?} in TextView {}, not doing anything in response to draw request.", tv.get_canvas_gid(), tv.text);
                            // silently fail if a bogus Gid is given???
                        }
                    },
                    TextOp::ComputeBounds => {
                        log::trace!("render request for {:?}", tv);
                        tv.set_dry_run(true);

                        if tv.clip_rect.is_none() {
                            // fill in the clip rect from the canvas
                            if let Some(canvas) = canvases.get_mut(&tv.get_canvas_gid()) {
                                // set the clip rectangle according to the canvas' location
                                let mut base_clip_rect = canvas.clip_rect();
                                base_clip_rect.normalize();
                                tv.clip_rect = Some(base_clip_rect.into());
                            } else {
                                info!("bogus GID {:?} in TextView {}, not doing anything in response to draw request.", tv.get_canvas_gid(), tv.text);
                                // silently fail if a bogus Gid is given???
                                continue;
                            }
                        }
                        let mut tv_clone = tv.clone();
                        // issue the draw command
                        gfx.draw_textview(&mut tv_clone).expect("text view draw could not complete.");
                        // copy back the fields that we want to be mutable
                        log::trace!("got computed cursor of {:?}, bounds {:?}", tv_clone.cursor, tv_clone.bounds_computed);
                        tv.cursor = tv_clone.cursor;
                        tv.bounds_computed = tv_clone.bounds_computed;

                        let ret = api::Return::RenderReturn(tv);
                        buffer.replace(ret).unwrap();
                    }
                };
            }
            Some(Opcode::SetCanvasBounds) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut cb = buffer.to_original::<SetCanvasBoundsRequest, _>().unwrap();
                log::trace!("SetCanvasBoundsRequest {:?}", cb);

                let granted = if cb.token_type == TokenType::Gam {
                    context_mgr.set_canvas_height(&gfx, cb.token, cb.requested.y, &status_canvas, &mut canvases)
                } else {
                    context_mgr.set_canvas_height_app_token(&gfx, cb.token, cb.requested.y, &status_canvas, &mut canvases)
                };
                if granted.is_some() {
                    // recompute the canvas orders based on the new layout
                    let recomp_canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));
                    canvases = recomp_canvases;
                    log::trace!("canvas bounds redraw");
                    context_mgr.redraw().expect("can't redraw after new canvas bounds");
                }
                cb.granted = granted;
                let ret = api::Return::SetCanvasBoundsReturn(cb);
                log::trace!("returning {:?}", cb);
                buffer.replace(ret).unwrap();
            }
            Some(Opcode::RequestContentCanvas) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let req = buffer.to_original::<[u32; 4], _>().unwrap();
                log::trace!("RequestContentCanvas {:?}", req);

                let ret = api::Return::ContentCanvasReturn(context_mgr.get_content_canvas(req));
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
                        log::debug!("attempt to draw Object on non-drawable canvas. Not fatal, but request ignored: {:?}", obj);
                    }
                } else {
                    info!("bogus GID in Object, not doing anything in response to draw request.");
                }
                log::trace!("leaving RenderObject");
            }
            Some(Opcode::ClaimToken) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tokenclaim = buffer.to_original::<TokenClaim, _>().unwrap();
                tokenclaim.token = context_mgr.claim_token(tokenclaim.name.as_str().unwrap());
                buffer.replace(tokenclaim).unwrap();
            },
            Some(Opcode::TrustedInitDone) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if context_mgr.allow_untrusted_code() {
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::RegisterUx) => {
                let mut buffer = unsafe{ Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let registration = buffer.to_original::<UxRegistration, _>().unwrap();

                // note that we are currently assigning all Ux registrations a trust level consistent with a boot context (ultimately trusted)
                // this needs to be modified later on once we allow post-boot apps to be created
                let token = context_mgr.register(&gfx, &trng, &status_canvas, &mut canvases,
                    registration);

                // compute what canvases are drawable
                // this _replaces_ the original canvas structure, to avoid complications of tracking mutable references through compound data structures
                // this is broken into two steps because of https://github.com/rust-lang/rust/issues/71126
                canvases = recompute_canvases(&canvases, Rectangle::new(Point::new(0, 0), screensize));

                buffer.replace(Return::UxToken(token)).unwrap();
            },
            Some(Opcode::SetAudioOpcode) => {
                let buffer = unsafe{ Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let audio_op = buffer.to_original::<SetAudioOpcode, _>().unwrap();
                context_mgr.set_audio_op(audio_op);
            },
            Some(Opcode::InputLine) => {
                // receive the keyboard input and pass it on to the context with focus
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let inputline = buffer.to_original::<String::<4000>, _>().unwrap();
                log::debug!("received input line, forwarding on... {}", inputline);
                context_mgr.forward_input(inputline).expect("couldn't forward input line to focused app");
                log::debug!("returned from forward_input");
            },
            Some(Opcode::KeyboardEvent) => msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                let keys = [
                    if let Some(a) = core::char::from_u32(k1 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k2 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k3 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                    if let Some(a) = core::char::from_u32(k4 as u32) {
                        a
                    } else {
                        '\u{0000}'
                    },
                ];
                context_mgr.key_event(keys, &gfx, &mut canvases);
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, ena, _,  _,  _, {
                if ena != 0 { context_mgr.vibe(true) }
                else { context_mgr.vibe(false) }
            }),
            Some(Opcode::RevertFocus) => {
                context_mgr.revert_focus(&gfx, &mut canvases);
                xous::return_scalar(msg.sender, 0).expect("couldn't unblock caller");
            },
            Some(Opcode::RevertFocusNb) => {
                context_mgr.revert_focus(&gfx, &mut canvases);
            },
            Some(Opcode::RequestFocus) => msg_blocking_scalar_unpack!(msg, t0, t1, t2, t3, {
                // TODO: add some limitations around who can request focus
                // for now, it's the boot set so we just trust the requestor
                context_mgr.activate(&gfx, &mut canvases, [t0 as u32, t1 as u32, t2 as u32, t3 as u32], true);

                // this is a blocking scalar, so return /something/ so we know to move on
                xous::return_scalar(msg.sender, 1).expect("couldn't confirm focus activation");
            }),
            Some(Opcode::QueryGlyphProps) => msg_blocking_scalar_unpack!(msg, style, _, _, _, {
                let height = gfx.glyph_height_hint(GlyphStyle::from(style)).expect("couldn't query glyph height from gfx");
                xous::return_scalar(msg.sender, height).expect("could not return QueryGlyphProps request");
            }),
            Some(Opcode::RedrawIme) => {
                context_mgr.redraw_imef().expect("couldn't redraw the IMEF");
            },
            Some(Opcode::SwitchToApp) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let switchapp = buffer.to_original::<SwitchToApp, _>().unwrap();
                log::debug!("trying to switch to {:?} with token {:?}", switchapp.app_name.as_str().unwrap(), switchapp.token);

                if let Some(new_app_token) = context_mgr.find_app_token_by_name(switchapp.app_name.as_str().unwrap()) {
                    if let Some(menu_token) = context_mgr.find_app_token_by_name(MAIN_MENU_NAME) {
                        if menu_token == switchapp.token {
                            context_mgr.activate(&gfx, &mut canvases, new_app_token, false);
                            continue;
                        }
                    }
                    if let Some(modal_token) = context_mgr.find_app_token_by_name(ROOTKEY_MODAL_NAME) {
                        if modal_token == switchapp.token {
                            context_mgr.activate(&gfx, &mut canvases, new_app_token, false);
                            continue;
                        }
                    }
                    if let Some(token) = context_mgr.find_app_token_by_name(gam::STATUS_BAR_NAME) {
                        if token == switchapp.token {
                            context_mgr.activate(&gfx, &mut canvases, new_app_token, true);
                            continue;
                        }
                    }
                }
            },
            Some(Opcode::RaiseMenu) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let menu_name = buffer.to_original::<String::<128>, _>().unwrap();
                log::debug!("got request to raise menu {}", menu_name);
                context_mgr.raise_menu(menu_name.as_str().unwrap(), &gfx, &mut canvases);
            },
            Some(Opcode::Devboot) => msg_scalar_unpack!(msg, ena, _,  _,  _, {
                if ena != 0 { gfx.set_devboot(true).expect("couldn't send devboot message"); }
                else { gfx.set_devboot(false).expect("couldn't send devboot message"); }
            }),
            Some(Opcode::TestPattern) => msg_blocking_scalar_unpack!(msg, duration_ms, _, _, _, {
                if !did_test {
                    did_test = true;
                    let checked_duration = if duration_ms > 60_000 {
                        60_000
                    } else {
                        duration_ms
                    };
                    gfx.selftest(checked_duration);
                }
                xous::return_scalar(msg.sender, 1).expect("couldn't ack self test");
            }),
            Some(Opcode::Quit) => break,
            None => {log::error!("unhandled message {:?}", msg);}
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
