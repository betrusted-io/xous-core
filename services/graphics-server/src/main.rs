#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

mod backend;
use backend::XousDisplay;

mod op;

mod logo;
mod poweron;
mod sleep_note;

use api::{DrawStyle, PixelColor, Rectangle, TextBounds, RoundedRectangle, Point, TextView, Line, Circle};
use api::{Opcode, ClipObject, ClipObjectType};
use blitstr_ref as blitstr;
use blitstr::GlyphStyle;

use num_traits::FromPrimitive;
use xous_ipc::Buffer;
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack, MemoryRange};

mod fontmap;
use api::{BulkRead, ArchivedBulkRead};

fn draw_boot_logo(display: &mut XousDisplay) {
    display.blit_screen(poweron::LOGO_MAP);
}

#[cfg(target_os = "none")]
fn map_fonts() -> MemoryRange {
    log::trace!("mapping fonts");
    // this maps an extra page if the total length happens to fall on a 4096-byte boundary, but this is ok
    // because the reserved area is much larger
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    log::trace!("requesting map of length 0x{:08x} at 0x{:08x}", fontlen, fontmap::FONT_BASE);
    let fontregion = xous::syscall::map_memory(
        xous::MemoryAddress::new(fontmap::FONT_BASE),
        None,
        fontlen as usize,
        xous::MemoryFlags::R,
    ).expect("couldn't map fonts");
    log::info!("font base at virtual 0x{:08x}, len of 0x{:08x}", fontregion.as_ptr() as usize, usize::from(fontregion.len()));

    log::trace!("mapping regular font to 0x{:08x}", fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize);
    blitstr::map_font(blitstr::GlyphData::Emoji((fontregion.as_ptr() as usize + fontmap::EMOJI_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Hanzi((fontregion.as_ptr() as usize + fontmap::HANZI_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Regular((fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Small((fontregion.as_ptr() as usize + fontmap::SMALL_OFFSET) as usize));
    blitstr::map_font(blitstr::GlyphData::Bold((fontregion.as_ptr() as usize + fontmap::BOLD_OFFSET) as usize));

    fontregion
}

#[cfg(not(target_os = "none"))]
fn map_fonts() -> MemoryRange {
    // does nothing
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32) & 0xFFFF_F000) + 0x1000 + 8;
    let fontregion = xous::syscall::map_memory(
        None,
        None,
        fontlen as usize,
        xous::MemoryFlags::R,
    ).expect("couldn't map dummy memory for fonts");

    fontregion
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    let debugtv = false;
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // these connections should be established:
    // - GAM
    // - keyrom (for verifying font maps)
    let sid = xns.register_name(api::SERVER_NAME_GFX, Some(2)).expect("can't register server");
    log::trace!("Server listening on address {:?}", sid);

    // Create a new monochrome simulator display.
    let mut display = XousDisplay::new();

    draw_boot_logo(&mut display);

    let fontregion = map_fonts();

    let mut use_sleep_note = true;
    if false {
        // leave this test case around
        // for some reason, the top right quadrant draws an extra pixel inside the fill area
        // when a fill color of "Light" is specified. However, if `None` fill is specified, it
        // works correctly. This is really puzzling, because the test for filled drawing happens
        // after the test for border drawing.
        use api::Point;
        let mut r = Rectangle::new(Point::new(20, 200), Point::new(151, 301));
        r.style = DrawStyle {
            fill_color: Some(PixelColor::Light),
            stroke_color: Some(PixelColor::Dark),
            stroke_width: 1,
        };
        let rr = RoundedRectangle::new(r, 16);
        op::rounded_rectangle(display.native_buffer(), rr, None);
    }

    let screen_clip = Rectangle::new(Point::new(0,0), display.screen_size());

    display.redraw();

    // register a suspend/resume listener
    let sr_cid = xous::connect(sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut bulkread = BulkRead::default(); // holding buffer for bulk reads; wastes ~8k when not in use, but saves a lot of copy/init for each iteration of the read
    loop {
        let mut msg = xous::receive_message(sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                display.suspend(use_sleep_note);
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                display.resume(use_sleep_note);
            }),
            Some(Opcode::SetSleepNote) => xous::msg_scalar_unpack!(msg, set_use, _, _, _, {
                if set_use == 0 {
                    use_sleep_note = false;
                } else {
                    use_sleep_note = true;
                }
            }),
            Some(Opcode::DrawClipObject) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let obj = buffer.to_original::<ClipObject, _>().unwrap();
                log::trace!("DrawClipObject {:?}", obj);
                match obj.obj {
                    ClipObjectType::Line(line) => {
                        op::line(display.native_buffer(), line, Some(obj.clip), false);
                    },
                    ClipObjectType::XorLine(line) => {
                        op::line(display.native_buffer(), line, Some(obj.clip), true);
                    },
                    ClipObjectType::Circ(circ) => {
                        op::circle(display.native_buffer(), circ, Some(obj.clip));
                    },
                    ClipObjectType::Rect(rect) => {
                        op::rectangle(display.native_buffer(), rect, Some(obj.clip));
                    },
                    ClipObjectType::RoundRect(rr) => {
                        op::rounded_rectangle(display.native_buffer(), rr, Some(obj.clip));
                    }
                }
            }
            Some(Opcode::DrawTextView) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut tv = buffer.to_original::<TextView, _>().unwrap();

                if tv.clip_rect.is_none() { continue } // if no clipping rectangle is specified, nothing to draw
                let clip_rect = tv.clip_rect.unwrap(); // this is the clipping rectangle of the canvas
                let screen_offset: Point = tv.clip_rect.unwrap().tl; // this is the translation vector to and from screen space

                let paintfn = if tv.dry_run() {
                    if debugtv { log::trace!("(TV): doing dry run"); }
                    blitstr::simulate_char
                } else {
                    if debugtv { log::trace!("(TV): doing live run"); }
                    blitstr::xor_char
                };

                // first compute the bounding box, if it isn't computed
                if tv.bounds_computed.is_none() {
                    match tv.bounds_hint {
                        TextBounds::BoundingBox(r) => {
                            tv.bounds_computed = Some(r);
                        },
                        TextBounds::GrowableFromBr(br, width) => {
                            if !clip_rect.intersects_point(br) {
                                continue;
                            }
                            // assume: clip_rect is the total canvas area we could draw
                            // assume: br is the point we want to extend the drawable text bubble on
                            let checkedwidth: i16 = if width as i16 <= (br.x - clip_rect.tl.x) {
                                width as _
                            } else {
                                (br.x - clip_rect.tl.x) as _
                            };
                            // first, create a clip that's the width of the growable, but as big as the height of the screen
                            let clip = blitstr::ClipRect::new(0, 0, checkedwidth as _, display.screen_size().y as _);
                            let mut c = blitstr::Cursor::new(0, 0, 0);
                            // now simulate the string painting
                            blitstr::paint_str(
                                display.native_buffer(),
                                clip,
                                &mut c,
                                tv.style.into(),
                                tv.text.as_str().unwrap(),
                                false,
                                None,
                                false,
                                blitstr::simulate_char
                            );
                            // the resulting cursor position + line_height + margin is the height of the bounds
                            let checkedheight: i16 = if (c.pt.y as i16 + c.line_height as i16 + (tv.margin.y as i16) * 2) <= (br.y - clip_rect.tl.y as i16) {
                                c.pt.y as i16 + c.line_height as i16 + 2 * tv.margin.y
                            } else {
                                br.y as i16 - clip_rect.tl.y as i16
                            };
                            // if less than one line of text, shrink the box
                            let finalwidth = if c.pt.y == 0 {
                                if c.pt.x as i16 + c.line_height as i16 + tv.margin.x < checkedwidth {
                                    c.pt.x as i16 + c.line_height as i16 + tv.margin.x
                                } else {
                                    checkedwidth
                                }
                            } else {
                                checkedwidth
                            };
                            let tl = Point::new(br.x - finalwidth, br.y - checkedheight);
                            if clip_rect.intersects_point(tl) {
                                tv.bounds_computed = Some(Rectangle::new(
                                    tl,
                                    br
                                ));
                            } else {
                                tv.bounds_computed = None;
                            }
                        },
                        TextBounds::GrowableFromTl(tl, width) => {
                            log::trace!("growablefromtl, cr: {:?}", clip_rect);
                            if !clip_rect.intersects_point(tl) {
                                log::trace!("didn't intersect: clip_rect {:?}, tl {:?}", clip_rect, tl);
                                continue;
                            }
                            // assume: clip_rect is the total canvas area we could draw
                            // assume: tl is the point we want to extend the drawable text bubble on
                            let checkedwidth: i16 = if width as i16 <= (clip_rect.br.x - tl.x) {
                                width as _
                            } else {
                                (clip_rect.br.x - tl.x) as _
                            };
                            // first, create a clip that's the width of the growable, but as big as the height of the screen
                            let clip = blitstr::ClipRect::new(0, 0, checkedwidth as _, display.screen_size().y as _);
                            let mut c = blitstr::Cursor::new(0, 0, 0);
                            // now simulate the string painting
                            blitstr::paint_str(
                                display.native_buffer(),
                                clip,
                                &mut c,
                                tv.style.into(),
                                tv.text.as_str().unwrap(),
                                false,
                                None,
                                false,
                                blitstr::simulate_char
                            );
                            // the resulting cursor position + line_height is the height of the bounds
                            let checkedheight: i16 = c.pt.y as i16 + c.line_height as i16;

                            // if less than one line of text, shrink the box
                            let finalwidth = if c.pt.y == 0 {
                                if c.pt.x as i16 + c.line_height as i16 + tv.margin.x < checkedwidth {
                                    c.pt.x as i16 + c.line_height as i16 + tv.margin.x
                                } else {
                                    checkedwidth
                                }
                            } else {
                                checkedwidth
                            };
                            let br = Point::new(tl.x + finalwidth, tl.y + checkedheight);
                            log::trace!("br: {:?}, comp w: {}, comp h: {}", br, finalwidth, checkedheight);
                            if clip_rect.intersects_point(br) {
                                tv.bounds_computed = Some(Rectangle::new(
                                    tl,
                                    br,
                                ));
                                log::trace!("intersects, bounds_computed: {:?}", tv.bounds_computed);
                            } else {
                                log::trace!("does not intersect, clip_rect: {:?}, br: {:?}", clip_rect, br);
                                tv.bounds_computed = None;
                            }
                        },
                        TextBounds::GrowableFromBl(bl, width) => {
                            if !clip_rect.intersects_point(bl) {
                                continue;
                            }
                            // assume: clip_rect is the total canvas area we could draw
                            // assume: bl is the point we want to extend the drawable text bubble on
                            let checkedwidth: i16 = if width as i16 <= (clip_rect.br.x - bl.x) {
                                width as _
                            } else {
                                (clip_rect.br.x - bl.x) as _
                            };
                            // first, create a clip that's the width of the growable, but as big as the height of the screen
                            let clip = blitstr::ClipRect::new(0, 0, checkedwidth as _, display.screen_size().y as _);
                            let mut c = blitstr::Cursor::new(0, 0, 0);
                            // now simulate the string painting
                            blitstr::paint_str(
                                display.native_buffer(),
                                clip,
                                &mut c,
                                tv.style.into(),
                                tv.text.as_str().unwrap(),
                                false,
                                None,
                                false,
                                blitstr::simulate_char
                            );
                            // the resulting cursor position + line_height is the height of the bounds
                            let checkedheight: i16 = if (c.pt.y as i16 + c.line_height as i16 + 2 * tv.margin.y as i16) <= (bl.y as i16 - clip_rect.tl.y as i16) {
                                c.pt.y as i16 + c.line_height as i16 + 2 * tv.margin.y
                            } else {
                                bl.y as i16 - clip_rect.tl.y as i16
                            };

                            // if less than one line of text, shrink the box
                            let finalwidth = if c.pt.y == 0 {
                                if c.pt.x as i16 + c.line_height as i16 + tv.margin.x < checkedwidth {
                                    c.pt.x as i16 + c.line_height as i16 + tv.margin.x
                                } else {
                                    checkedwidth
                                }
                            } else {
                                checkedwidth
                            };
                            let tl = Point::new(bl.x, bl.y - checkedheight);
                            if clip_rect.intersects_point(tl) {
                                tv.bounds_computed = Some(Rectangle::new(
                                    tl,
                                    Point::new(bl.x + finalwidth, bl.y)
                                ));
                            } else {
                                tv.bounds_computed = None
                            }
                        },
                    }
                }
                if debugtv { log::info!("(TV): computed bounds {:?}", tv.bounds_computed); }
                if tv.bounds_computed.is_none() {
                    // the bounds weren't valid, so don't draw
                    continue;
                }

                // clear the bounding box if requested
                let mut clear_rect = tv.bounds_computed.unwrap();

                // move things into screen coordinates
                clear_rect.translate(screen_offset);

                let bordercolor = if tv.draw_border {
                    Some(PixelColor::Dark)
                } else {
                    None
                };
                let borderwidth: i16 = if tv.draw_border {
                    tv.border_width as i16
                } else {
                    0
                };
                let fillcolor = if tv.clear_area || tv.invert {
                    if tv.invert {
                        Some(PixelColor::Dark)
                    } else {
                        Some(PixelColor::Light)
                    }
                } else {
                    None
                };

                clear_rect.style = DrawStyle {
                    fill_color: fillcolor,
                    stroke_color: bordercolor,
                    stroke_width: borderwidth,
                };
                if !tv.dry_run() {
                    if tv.rounded_border.is_some() {
                        op::rounded_rectangle(display.native_buffer(),
                            RoundedRectangle::new(clear_rect, tv.rounded_border.unwrap() as _), Some(clear_rect));
                    } else {
                        if debugtv { log::trace!("(TV): clearing rectangle {:?}", clear_rect); }
                        op::rectangle(display.native_buffer(), clear_rect, tv.clip_rect);
                    }
                }

                // compute the final clipping region for the string
                clear_rect.margin(tv.margin);
                let cr = match clear_rect.clip_with(screen_clip) {
                    Some(r) => r,
                    _ => continue, // don't draw anything if somehow this doesn't fit in the creen.
                };

                let mut ref_cursor = blitstr::Cursor::from_top_left_of(cr.into());
                if debugtv { log::trace!("(TV): paint_str with {:?} | {:?} | {:?} | {:?} len: {}", cr, ref_cursor, tv.style, tv.text, tv.text.as_str().unwrap().len()); }
                log::debug!("{}", tv);
                let do_xor = tv.invert;
                blitstr::paint_str(
                    display.native_buffer(),
                    cr.into(),
                    &mut ref_cursor,
                    tv.style.into(),
                    tv.text.as_str().unwrap(),
                    do_xor,
                    tv.insertion,
                    tv.ellipsis,
                    paintfn
                );
                // translate the cursor return value back to canvas coordinates
                tv.cursor = blitstr::Cursor {
                    pt: blitstr::Pt::new(
                        ref_cursor.pt.x - screen_offset.x as i32,
                        ref_cursor.pt.y - screen_offset.y as i32,
                    ),
                    line_height: ref_cursor.line_height,
                };
                if debugtv{log::trace!("(TV): returning cursor of {:?}", tv.cursor);}

                // pack our data back into the buffer to return
                buffer.replace(tv).unwrap();
            }
            Some(Opcode::Flush) => {
                display.update();
                display.redraw();
            }
            Some(Opcode::Clear) => {
                let mut r = Rectangle::full_screen();
                r.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 0);
                op::rectangle(display.native_buffer(), r, screen_clip.into())
            }
            Some(Opcode::Line) => msg_scalar_unpack!(msg, p1, p2, style, _, {
                let l = Line::new_with_style(Point::from(p1), Point::from(p2), DrawStyle::from(style));
                op::line(display.native_buffer(), l, screen_clip.into(), false);
            }),
            Some(Opcode::Rectangle) => msg_scalar_unpack!(msg, tl, br, style, _, {
                let r = Rectangle::new_with_style(Point::from(tl), Point::from(br), DrawStyle::from(style));
                op::rectangle(display.native_buffer(), r, screen_clip.into());
            }),
            Some(Opcode::RoundedRectangle) => msg_scalar_unpack!(msg, tl, br, style, r, {
                let rr = RoundedRectangle::new(Rectangle::new_with_style(Point::from(tl), Point::from(br), DrawStyle::from(style)), r as _);
                op::rounded_rectangle(display.native_buffer(), rr, screen_clip.into());
            }),
            Some(Opcode::Circle) => msg_scalar_unpack!(msg, center, radius, style, _, {
                let c = Circle::new_with_style(Point::from(center), radius as _, DrawStyle::from(style));
                op::circle(display.native_buffer(), c, screen_clip.into());
            }),
            Some(Opcode::ScreenSize) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let pt = display.screen_size();
                xous::return_scalar2(msg.sender,
                    pt.x as usize,
                    pt.y as usize
                ).expect("couldn't return ScreenSize request");
            }),
            Some(Opcode::QueryGlyphProps) => msg_blocking_scalar_unpack!(msg, style, _, _, _, {
                let glyph = GlyphStyle::from(style);
                xous::return_scalar2(msg.sender,
                    glyph.into(),
                    blitstr::glyph_to_height_hint(glyph)
                ).expect("could not return QueryGlyphProps request");
            }),
            Some(Opcode::DrawSleepScreen) => msg_scalar_unpack!(msg, _, _, _, _, {
                display.blit_screen(logo::LOGO_MAP);
                display.update();
                display.redraw();
            }),
            Some(Opcode::Devboot) => msg_scalar_unpack!(msg, ena, _,  _,  _, {
                if ena != 0 { display.set_devboot(true); }
                else { display.set_devboot(false); }
            }),
            Some(Opcode::RestartBulkRead) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                bulkread.from_offset = 0;
                xous::return_scalar(msg.sender, 0).expect("couldn't ack that bulk read pointer was reset");
            }),
            Some(Opcode::BulkReadFonts) => {
                let fontlen = fontmap::FONT_TOTAL_LEN as u32 + 8;
                let mut buf = unsafe{ Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                //let mut bulkread = buf.as_flat::<BulkRead, _>().unwrap(); // try to skip the copy/init step by using a persistent structure
                let fontslice = fontregion.as_slice::<u8>();
                assert!(fontlen <= fontslice.len() as u32);
                if bulkread.from_offset >= fontlen {
                    log::error!("BulkReadFonts attempt to read out of bound on the font area; ignoring!");
                    continue
                }
                let readlen = if bulkread.from_offset + bulkread.buf.len() as u32 > fontlen {
                    // returns what is readable of the last bit; anything longer than the fontlen is undefined/invalid
                    fontlen as usize - bulkread.from_offset as usize
                } else {
                    bulkread.buf.len()
                };
                for (&src, dst) in fontslice[bulkread.from_offset as usize .. bulkread.from_offset as usize + readlen].iter()
                    .zip(bulkread.buf.iter_mut()) {
                        *dst = src;
                }
                bulkread.len = readlen as u32;
                bulkread.from_offset += readlen as u32;
                buf.replace(bulkread).unwrap();
            }
            Some(Opcode::Quit) => break,
            None => {log::error!("received opcode scalar that is not handled");}
        }
        display.update();
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
