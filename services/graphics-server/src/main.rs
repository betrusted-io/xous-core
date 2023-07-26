#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

mod backend;
use backend::XousDisplay;

mod op;

mod logo;
mod poweron;
mod sleep_note;

use api::*;

mod blitstr2;
mod wordwrap;
#[macro_use]
mod style_macros;


use num_traits::FromPrimitive;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, MemoryRange};
use xous_ipc::Buffer;

mod fontmap;
use api::BulkRead;

#[cfg(any(feature="precursor", feature="renode"))] // only install for hardware targets; hosted mode uses host's panic handler
mod panic;

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::wordwrap::*;
use core::ops::Add;

#[cfg(feature = "gfx-testing")]
mod testing;

fn draw_boot_logo(display: &mut XousDisplay) {
    display.blit_screen(&poweron::LOGO_MAP);
}

#[cfg(any(feature="precursor", feature="renode"))]
fn map_fonts() -> MemoryRange {
    log::trace!("mapping fonts");
    // this maps an extra page if the total length happens to fall on a 4096-byte boundary, but this is ok
    // because the reserved area is much larger
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    log::trace!(
        "requesting map of length 0x{:08x} at 0x{:08x}",
        fontlen,
        fontmap::FONT_BASE
    );
    let fontregion = xous::syscall::map_memory(
        xous::MemoryAddress::new(fontmap::FONT_BASE),
        None,
        fontlen as usize,
        xous::MemoryFlags::R,
    )
    .expect("couldn't map fonts");
    log::info!(
        "font base at virtual 0x{:08x}, len of 0x{:08x}",
        fontregion.as_ptr() as usize,
        usize::from(fontregion.len())
    );

    log::trace!(
        "mapping regular font to 0x{:08x}",
        fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize
    );
    blitstr2::fonts::bold::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::BOLD_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::emoji::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::EMOJI_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::ja::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::JA_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::kr::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::KR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::mono::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::MONO_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::regular::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::small::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::SMALL_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::zh::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::ZH_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::tall::GLYPH_LOCATION.store((fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize) as u32, Ordering::SeqCst);

    fontregion
}

#[cfg(not(target_os = "xous"))]
fn map_fonts() -> MemoryRange {
    // does nothing
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    let fontregion = xous::syscall::map_memory(None, None, fontlen as usize, xous::MemoryFlags::R)
        .expect("couldn't map dummy memory for fonts");

    fontregion
}
fn main () -> ! {
    // Some operating systems and GUI frameworks don't allow creating an event
    // loop from a thread other than TID 1. Let the backend claim this thread
    // if this may be the case.
    backend::claim_main_thread(move |main_thread_token| {
        #[cfg(not(feature = "ditherpunk"))]
        wrapped_main(main_thread_token);

        #[cfg(feature="ditherpunk")]
        let stack_size = 1024 * 1024;
        #[cfg(feature="ditherpunk")]
        std::thread::Builder::new()
            .stack_size(stack_size)
            .spawn(move || wrapped_main(main_thread_token))
            .unwrap()
            .join()
            .unwrap()
    })
}
fn wrapped_main(main_thread_token: backend::MainThreadToken) -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let mut display = XousDisplay::new(main_thread_token);
    draw_boot_logo(&mut display); // bring this up as soon as possible
    let fontregion = map_fonts();

    // install the graphical panic handler. It won't catch really early panics, or panics in this crate,
    // but it'll do the job 90% of the time and it's way better than having none at all.
    let is_panic = Arc::new(AtomicBool::new(false));
    #[cfg(any(feature="precursor", feature="renode"))] // only install for hardware targets; hosted mode uses host's panic handler
    {
        let (hwfb, control) = unsafe{
            // this is safe because we are extracting these for use in a Mutex-protected panic handler
            display.hw_regs()
        };
        panic::panic_handler_thread(is_panic.clone(), hwfb, control);
    }

    let xns = xous_names::XousNames::new().unwrap();
    // these connections should be established:
    // - GAM
    // - keyrom (for verifying font maps)
    #[cfg(any(feature="precursor", feature="renode"))]
    let sid = xns
        .register_name(api::SERVER_NAME_GFX, Some(2))
        .expect("can't register server");
    #[cfg(not(target_os = "xous"))]
    let sid = xns
        .register_name(api::SERVER_NAME_GFX, Some(1))
        .expect("can't register server");

    let screen_clip = Rectangle::new(Point::new(0, 0), display.screen_size());

    display.redraw();

    // register a suspend/resume listener
    let sr_cid = xous::connect(sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Later), &xns, Opcode::SuspendResume as u32, sr_cid)
        .expect("couldn't create suspend/resume object");

    let mut bulkread = BulkRead::default(); // holding buffer for bulk reads; wastes ~8k when not in use, but saves a lot of copy/init for each iteration of the read

    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

    #[cfg(feature = "gfx-testing")]
    testing::tests();
    loop {
        if !is_panic.load(Ordering::Relaxed) { // non-panic graphics operations if we are in a panic situation
            let mut msg = xous::receive_message(sid).unwrap();
            log::trace!("Message: {:?}", msg);
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                    display.suspend();
                    susres
                        .suspend_until_resume(token)
                        .expect("couldn't execute suspend/resume");
                    display.resume();
                }),
                Some(Opcode::DrawClipObject) => {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let obj = buffer.to_original::<ClipObject, _>().unwrap();
                    log::trace!("DrawClipObject {:?}", obj);
                    match obj.obj {
                        ClipObjectType::Line(line) => {
                            op::line(display.native_buffer(), line, Some(obj.clip), false);
                        }
                        ClipObjectType::XorLine(line) => {
                            op::line(display.native_buffer(), line, Some(obj.clip), true);
                        }
                        ClipObjectType::Circ(circ) => {
                            op::circle(display.native_buffer(), circ, Some(obj.clip));
                        }
                        ClipObjectType::Rect(rect) => {
                            op::rectangle(display.native_buffer(), rect, Some(obj.clip));
                        }
                        ClipObjectType::RoundRect(rr) => {
                            op::rounded_rectangle(display.native_buffer(), rr, Some(obj.clip));
                        }
                        #[cfg(feature="ditherpunk")]
                        ClipObjectType::Tile(tile) => {
                            op::tile(display.native_buffer(), tile, Some(obj.clip));
                        }
                    }
                }
                Some(Opcode::DrawClipObjectList) => {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let list_ipc = buffer.to_original::<ClipObjectList, _>().unwrap();
                    for maybe_item in list_ipc.list.iter() {
                        if let Some(obj) = maybe_item {
                            match obj.obj {
                                ClipObjectType::Line(line) => {
                                    op::line(display.native_buffer(), line, Some(obj.clip), false);
                                }
                                ClipObjectType::XorLine(line) => {
                                    op::line(display.native_buffer(), line, Some(obj.clip), true);
                                }
                                ClipObjectType::Circ(circ) => {
                                    op::circle(display.native_buffer(), circ, Some(obj.clip));
                                }
                                ClipObjectType::Rect(rect) => {
                                    op::rectangle(display.native_buffer(), rect, Some(obj.clip));
                                }
                                ClipObjectType::RoundRect(rr) => {
                                    op::rounded_rectangle(display.native_buffer(), rr, Some(obj.clip));
                                }
                                #[cfg(feature="ditherpunk")]
                                ClipObjectType::Tile(tile) => {
                                    op::tile(display.native_buffer(), tile, Some(obj.clip));
                                }
                            }
                        } else {
                            // stop at the first None entry -- if the sender packed the list with a hole in it, that's their bad
                            break;
                        }
                    }
                }
                Some(Opcode::DrawTextView) => {
                    let mut buffer = unsafe {
                        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                    };
                    let mut tv = buffer.to_original::<TextView, _>().unwrap();

                    if tv.clip_rect.is_none() {
                        continue;
                    } // if no clipping rectangle is specified, nothing to draw

                    // this is the clipping rectangle of the canvas in screen coordinates
                    let clip_rect = tv.clip_rect.unwrap();
                    // this is the translation vector to and from screen space
                    let screen_offset: Point = tv.clip_rect.unwrap().tl;

                    let typeset_extent = match tv.bounds_hint {
                        TextBounds::BoundingBox(r) =>
                            Pt::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2),
                        TextBounds::GrowableFromBr(br, width) =>
                            Pt::new(width as i16 - tv.margin.x * 2, br.y - tv.margin.y * 2),
                        TextBounds::GrowableFromBl(bl, width) =>
                            Pt::new(width as i16 - tv.margin.x * 2, bl.y - tv.margin.y * 2),
                        TextBounds::GrowableFromTl(tl, width) =>
                            Pt::new(width as i16 - tv.margin.x * 2, (clip_rect.br().y - clip_rect.tl().y - tl.y) - tv.margin.y * 2),
                        TextBounds::GrowableFromTr(tr, width) =>
                            Pt::new(width as i16 - tv.margin.x * 2, (clip_rect.br().y - clip_rect.tl().y - tr.y) - tv.margin.y * 2),
                        TextBounds::CenteredTop(r) =>
                            Pt::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2),
                        TextBounds::CenteredBot(r) =>
                            Pt::new(r.br().x - r.tl().x - tv.margin.x * 2, r.br().y - r.tl().y - tv.margin.y * 2),
                    };
                    let mut typesetter = Typesetter::setup(
                        tv.to_str(),
                        &typeset_extent,
                        &tv.style,
                        if let Some(i) = tv.insertion { Some(i as usize) } else { None }
                    );
                    let composition = typesetter.typeset(
                        if tv.ellipsis {
                            OverflowStrategy::Ellipsis
                        } else {
                            OverflowStrategy::Abort
                        }
                    );

                    let composition_top_left = match tv.bounds_hint {
                        TextBounds::BoundingBox(r) =>
                            r.tl().add(tv.margin),
                        TextBounds::GrowableFromBr(br, _width) =>
                            Point::new(br.x - (composition.bb_width() as i16 + tv.margin.x),
                            br.y - (composition.bb_height() as i16 + tv.margin.y)),
                        TextBounds::GrowableFromBl(bl, _width) =>
                            Point::new(bl.x + tv.margin.x, bl.y - (composition.bb_height() as i16 + tv.margin.y)),
                        TextBounds::GrowableFromTl(tl, _width) =>
                            tl.add(tv.margin),
                        TextBounds::GrowableFromTr(tr, _width) =>
                            Point::new(tr.x - (composition.bb_width() as i16 + tv.margin.x), tr.y + tv.margin.y),
                        TextBounds::CenteredTop(r) => {
                            if r.width() as i16 > composition.bb_width() {
                                r.tl().add(Point::new(
                                        (r.width() as i16 - composition.bb_width()) / 2, 0
                                ))
                            } else {
                                r.tl().add(tv.margin)
                            }
                        },
                        TextBounds::CenteredBot(r) => {
                            if r.width() as i16 > composition.bb_width() {
                                r.tl().add(Point::new(
                                        (r.width() as i16 - composition.bb_width()) / 2,
                                        if (r.height() as i16) > (composition.bb_height() + tv.margin.y) {
                                            (r.height() as i16) - (composition.bb_height() + tv.margin.y)
                                        } else {
                                            0
                                        }
                                ))
                            } else {
                                r.tl().add(tv.margin)
                            }
                        }
                    }
                    .add(screen_offset);

                    // compute the clear rectangle -- the border is already in screen coordinates, just add the margin around it
                    let mut clear_rect = match tv.bounds_hint {
                    TextBounds::BoundingBox(mut r)  => {
                            r.translate(screen_offset);
                            r
                    }
                    _ => {
                        // composition_top_left already had a screen_offset added when it was computed. just margin it out
                            let mut r = Rectangle::new(
                                composition_top_left,
                                composition_top_left.add(Point::new(composition.bb_width() as _, composition.bb_height() as _))
                            );
                            r.margin_out(tv.margin);
                            r
                        }
                    };

                    log::trace!("clip_rect: {:?}", clip_rect);
                    log::trace!("composition_top_left: {:?}", composition_top_left);
                    log::trace!("clear_rect: {:?}", clear_rect);
                    // draw the bubble/border and/or clear the background area
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
                            op::rounded_rectangle(
                                display.native_buffer(),
                                RoundedRectangle::new(clear_rect, tv.rounded_border.unwrap() as _),
                                tv.clip_rect,
                            );
                        } else {
                            op::rectangle(display.native_buffer(), clear_rect, tv.clip_rect);
                        }
                    }
                    // for now, if we're in braille mode, emit all text to the debug log so we can see it
                    //if cfg!(feature = "braille") {
                    //   log::info!("{}", tv);
                    //}

                    if !tv.dry_run() {
                        // note: make the clip rect `tv.clip_rect.unwrap()` if you want to debug wordwrapping artifacts; otherwise smallest_rect masks some problems
                        let smallest_rect = clear_rect.clip_with(tv.clip_rect.unwrap())
                            .unwrap_or(Rectangle::new(Point::new(0, 0), Point::new(0, 0,)));
                        composition.render(display.native_buffer(), composition_top_left, tv.invert, smallest_rect);
                    }
                    // type mismatch for now, replace this with a simple equals once we sort that out
                    tv.cursor.pt.x = composition.final_cursor().pt.x;
                    tv.cursor.pt.y = composition.final_cursor().pt.y;
                    tv.cursor.line_height = composition.final_cursor().line_height;

                    tv.bounds_computed = Some(
                        clear_rect
                    );
                    log::trace!("cursor ret {:?}, bounds ret {:?}", tv.cursor, tv.bounds_computed);
                    // pack our data back into the buffer to return
                    buffer.replace(tv).unwrap();
                }
                Some(Opcode::Flush) => {
                    log::trace!("***gfx flush*** redraw##");
                    display.redraw();
                }
                Some(Opcode::Clear) => {
                    let mut r = Rectangle::full_screen();
                    r.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 0);
                    op::rectangle(display.native_buffer(), r, screen_clip.into())
                }
                Some(Opcode::Line) => msg_scalar_unpack!(msg, p1, p2, style, _, {
                    let l =
                        Line::new_with_style(Point::from(p1), Point::from(p2), DrawStyle::from(style));
                    op::line(display.native_buffer(), l, screen_clip.into(), false);
                }),
                Some(Opcode::Rectangle) => msg_scalar_unpack!(msg, tl, br, style, _, {
                    let r = Rectangle::new_with_style(
                        Point::from(tl),
                        Point::from(br),
                        DrawStyle::from(style),
                    );
                    op::rectangle(display.native_buffer(), r, screen_clip.into());
                }),
                Some(Opcode::RoundedRectangle) => msg_scalar_unpack!(msg, tl, br, style, r, {
                    let rr = RoundedRectangle::new(
                        Rectangle::new_with_style(
                            Point::from(tl),
                            Point::from(br),
                            DrawStyle::from(style),
                        ),
                        r as _,
                    );
                    op::rounded_rectangle(display.native_buffer(), rr, screen_clip.into());
                }),
                #[cfg(feature="ditherpunk")]
                Some(Opcode::Tile) => {
                    let buffer =
                        unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let bm = buffer.to_original::<Tile, _>().unwrap();
                    op::tile(display.native_buffer(), bm, screen_clip.into());
                },
                Some(Opcode::Circle) => msg_scalar_unpack!(msg, center, radius, style, _, {
                    let c = Circle::new_with_style(
                        Point::from(center),
                        radius as _,
                        DrawStyle::from(style),
                    );
                    op::circle(display.native_buffer(), c, screen_clip.into());
                }),
                Some(Opcode::ScreenSize) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    let pt = display.screen_size();
                    xous::return_scalar2(msg.sender, pt.x as usize, pt.y as usize)
                        .expect("couldn't return ScreenSize request");
                }),
                Some(Opcode::QueryGlyphProps) => msg_blocking_scalar_unpack!(msg, style, _, _, _, {
                    let glyph = GlyphStyle::from(style);
                    xous::return_scalar2(
                        msg.sender,
                        glyph.into(),
                        glyph_to_height_hint(glyph),
                    )
                    .expect("could not return QueryGlyphProps request");
                }),
                Some(Opcode::DrawSleepScreen) => msg_scalar_unpack!(msg, _, _, _, _, {
                    display.blit_screen(&logo::LOGO_MAP);
                    display.redraw();
                }),
                Some(Opcode::DrawBootLogo) => msg_scalar_unpack!(msg, _, _, _, _, {
                    display.blit_screen(&poweron::LOGO_MAP);
                    display.redraw();
                }),
                Some(Opcode::Devboot) => msg_scalar_unpack!(msg, ena, _, _, _, {
                    if ena != 0 {
                        display.set_devboot(true);
                    } else {
                        display.set_devboot(false);
                    }
                }),
                Some(Opcode::RestartBulkRead) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    bulkread.from_offset = 0;
                    xous::return_scalar(msg.sender, 0)
                        .expect("couldn't ack that bulk read pointer was reset");
                }),
                Some(Opcode::BulkReadFonts) => {
                    // this also needs to reflect in root-keys/src/implementation.rs @ sign_loader()
                    let fontlen = fontmap::FONT_TOTAL_LEN as u32
                        + 16  // minver
                        + 16  // current ver
                        + 8;  // sig ver + len
                    let mut buf = unsafe {
                        Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                    };
                    //let mut bulkread = buf.as_flat::<BulkRead, _>().unwrap(); // try to skip the copy/init step by using a persistent structure
                    let fontslice = fontregion.as_slice::<u8>();
                    assert!(fontlen <= fontslice.len() as u32);
                    if bulkread.from_offset >= fontlen {
                        log::error!(
                            "BulkReadFonts attempt to read out of bound on the font area; ignoring!"
                        );
                        continue;
                    }
                    let readlen = if bulkread.from_offset + bulkread.buf.len() as u32 > fontlen {
                        // returns what is readable of the last bit; anything longer than the fontlen is undefined/invalid
                        fontlen as usize - bulkread.from_offset as usize
                    } else {
                        bulkread.buf.len()
                    };
                    for (&src, dst) in fontslice
                        [bulkread.from_offset as usize..bulkread.from_offset as usize + readlen]
                        .iter()
                        .zip(bulkread.buf.iter_mut())
                    {
                        *dst = src;
                    }
                    bulkread.len = readlen as u32;
                    bulkread.from_offset += readlen as u32;
                    buf.replace(bulkread).unwrap();
                }
                Some(Opcode::TestPattern) => msg_blocking_scalar_unpack!(msg, duration, _, _, _, {
                    let mut stashmem = xous::syscall::map_memory(
                        None,
                        None,
                        ((backend::FB_SIZE * 4) + 4096) & !4095,
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    ).expect("couldn't map stash frame buffer");
                    let stash = &mut stashmem.as_slice_mut()[..backend::FB_SIZE];
                    for (&src, dst) in display.as_slice().iter().zip(stash.iter_mut()) {
                        *dst = src;
                    }
                    for lines in 0..backend::FB_LINES { // mark all lines dirty
                        stash[lines * backend::FB_WIDTH_WORDS + (backend::FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                    }

                    let start_time = ticktimer.elapsed_ms();
                    let mut testmem = xous::syscall::map_memory(
                        None,
                        None,
                        ((backend::FB_SIZE * 4) + 4096) & !4095,
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    ).expect("couldn't map stash frame buffer");
                    let testpat = &mut testmem.as_slice_mut()[..backend::FB_SIZE];
                    const DWELL: usize = 1000;
                    while ticktimer.elapsed_ms() - start_time < duration as u64 {
                        // all black
                        for w in testpat.iter_mut() {
                            *w = 0;
                        }
                        for lines in 0..backend::FB_LINES { // mark dirty bits
                            testpat[lines * backend::FB_WIDTH_WORDS + (backend::FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                        }
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                        // all white
                        for w in testpat.iter_mut() {
                            *w = 0xFFFF_FFFF;
                        }
                        // dirty bits already set
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                        // vertical bars
                        for lines in 0..backend::FB_LINES {
                            for words in 0..backend::FB_WIDTH_WORDS {
                                testpat[lines * backend::FB_WIDTH_WORDS + words] = 0xaaaa_aaaa;
                            }
                        }
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                        for lines in 0..backend::FB_LINES {
                            for words in 0..backend::FB_WIDTH_WORDS {
                                testpat[lines * backend::FB_WIDTH_WORDS + words] = 0x5555_5555;
                            }
                        }
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                        // horiz bars
                        for lines in 0..backend::FB_LINES {
                            for words in 0..backend::FB_WIDTH_WORDS {
                                if lines % 2 == 0 {
                                    testpat[lines * backend::FB_WIDTH_WORDS + words] = 0x0;
                                } else {
                                    testpat[lines * backend::FB_WIDTH_WORDS + words] = 0xffff_ffff;
                                }
                            }
                            testpat[lines * backend::FB_WIDTH_WORDS + (backend::FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                        }
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                        for lines in 0..backend::FB_LINES {
                            for words in 0..backend::FB_WIDTH_WORDS {
                                if lines % 2 == 1 {
                                    testpat[lines * backend::FB_WIDTH_WORDS + words] = 0x0;
                                } else {
                                    testpat[lines * backend::FB_WIDTH_WORDS + words] = 0xffff_ffff;
                                }
                            }
                            testpat[lines * backend::FB_WIDTH_WORDS + (backend::FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                        }
                        display.blit_screen(testpat);
                        display.redraw();
                        ticktimer.sleep_ms(DWELL).unwrap();

                    }
                    display.blit_screen(stash);

                    xous::return_scalar(msg.sender, duration).expect("couldn't ack test pattern");
                }),
                Some(Opcode::Stash) => {
                    display.stash();
                    match msg.body { // ack the message if it's a blocking scalar
                        xous::Message::BlockingScalar(_) => xous::return_scalar(msg.sender, 1).unwrap(),
                        _ => ()
                    }
                }
                Some(Opcode::Pop) => {
                    display.pop();
                    match msg.body { // ack the message if it's a blocking scalar
                        xous::Message::BlockingScalar(_) => xous::return_scalar(msg.sender, 1).unwrap(),
                        _ => ()
                    }
                }
                Some(Opcode::Quit) => break,
                None => {
                    log::error!("received opcode scalar that is not handled");
                }
            }
        } else {
            // this is effectively an abort, because this is long enough for the WDT to fire and reboot the system
            ticktimer.sleep_ms(10_000).unwrap();
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
