#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod backend;
use backend::XousDisplay;
use ux_api::minigfx::*;
use ux_api::minigfx::{self, op};
use ux_api::platform::{FB_LINES, FB_SIZE, FB_WIDTH_WORDS};
use ux_api::service::api;
use ux_api::service::api::*;

mod logo;
#[cfg(not(feature = "cramium-soc"))]
mod poweron;
#[cfg(feature = "cramium-soc")]
mod poweron_bt;
#[cfg(feature = "cramium-soc")]
use poweron_bt as poweron;
mod sleep_note;

use blitstr2::fontmap;
use xous_ipc::Buffer;

#[cfg(any(feature = "precursor", feature = "renode", feature = "cramium-soc"))]
// only install for hardware targets; hosted mode uses host's panic handler
mod panic;

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(feature = "gfx-testing")]
mod testing;

fn draw_boot_logo(display: &mut XousDisplay) { display.blit_screen(&poweron::LOGO_MAP); }

#[cfg(any(feature = "precursor", feature = "renode"))]
fn map_fonts() -> xous::MemoryRange {
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
    blitstr2::fonts::bold::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::BOLD_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::emoji::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::EMOJI_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::ja::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::JA_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::kr::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::KR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::mono::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::MONO_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::regular::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::small::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::SMALL_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::zh::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::ZH_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::tall::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize) as u32, Ordering::SeqCst);

    fontregion
}

#[cfg(any(feature = "cramium-soc"))]
fn map_fonts() -> xous::MemoryRange {
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
    )
    .expect("couldn't map fonts");
    log::info!(
        "font base at virtual 0x{:08x}, len of 0x{:08x}",
        fontregion.as_ptr() as usize,
        usize::from(fontregion.len())
    );

    log::trace!(
        "mapping tall font to 0x{:08x}",
        fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize
    );
    blitstr2::fonts::bold::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::BOLD_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::emoji::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::EMOJI_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::mono::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::MONO_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::tall::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::TALL_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::regular::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::REGULAR_OFFSET as usize) as u32, Ordering::SeqCst);
    blitstr2::fonts::small::GLYPH_LOCATION
        .store((fontregion.as_ptr() as usize + fontmap::SMALL_OFFSET as usize) as u32, Ordering::SeqCst);

    fontregion
}

#[cfg(not(target_os = "xous"))]
fn map_fonts() -> xous::MemoryRange {
    // does nothing
    let fontlen: u32 = ((fontmap::FONT_TOTAL_LEN as u32 + 8) & 0xFFFF_F000) + 0x1000;
    let fontregion = xous::syscall::map_memory(None, None, fontlen as usize, xous::MemoryFlags::R)
        .expect("couldn't map dummy memory for fonts");

    fontregion
}
fn main() -> ! {
    // Some operating systems and GUI frameworks don't allow creating an event
    // loop from a thread other than TID 1. Let the backend claim this thread
    // if this may be the case.
    backend::claim_main_thread(move |main_thread_token| {
        #[cfg(not(feature = "ditherpunk"))]
        wrapped_main(main_thread_token);

        #[cfg(feature = "ditherpunk")]
        let stack_size = 1024 * 1024;
        #[cfg(feature = "ditherpunk")]
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
    #[cfg(any(feature = "precursor", feature = "renode"))]
    // only install for hardware targets; hosted mode uses host's panic handler
    {
        let (hwfb, control) = unsafe {
            // this is safe because we are extracting these for use in a Mutex-protected panic handler
            display.hw_regs()
        };
        panic::panic_handler_thread(is_panic.clone(), hwfb, control);
    }
    #[cfg(feature = "cramium-soc")]
    {
        // This is safe because the SPIM is finished with initialization, and the handler is
        // Mutex-protected.
        let hw_if = unsafe { display.hw_regs() };
        panic::panic_handler_thread(is_panic.clone(), hw_if);
    }

    let xns = xous_names::XousNames::new().unwrap();
    // these connections should be established:
    // - GAM
    // - keyrom (for verifying font maps)
    #[cfg(any(feature = "precursor", feature = "renode"))]
    let sid = xns.register_name(api::SERVER_NAME_GFX, Some(2)).expect("can't register server");
    #[cfg(not(target_os = "xous"))]
    let sid = xns.register_name(api::SERVER_NAME_GFX, Some(1)).expect("can't register server");
    #[cfg(feature = "cramium-soc")]
    let sid = {
        log::warn!(
            "Remember to notch the expected connections for graphics_server down to 1 after initial testing!"
        );
        // 2 for now -- one for GAM, one for testing
        xns.register_name(api::SERVER_NAME_GFX, Some(2)).expect("can't register server")
    };

    let screen_clip = Rectangle::new(Point::new(0, 0), display.screen_size());

    display.redraw();

    // register a suspend/resume listener
    #[cfg(not(feature = "cramium-soc"))]
    let sr_cid = xous::connect(sid).expect("couldn't create suspend callback connection");
    #[cfg(not(feature = "cramium-soc"))]
    let mut susres =
        susres::Susres::new(Some(susres::SuspendOrder::Later), &xns, GfxOpcode::SuspendResume as u32, sr_cid)
            .expect("couldn't create suspend/resume object");

    let mut bulkread = BulkRead::default(); // holding buffer for bulk reads; wastes ~8k when not in use, but saves a lot of copy/init for each iteration of the read

    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

    #[cfg(feature = "gfx-testing")]
    testing::tests();
    let mut msg_opt = None;
    loop {
        if !is_panic.load(Ordering::Relaxed) {
            xous::reply_and_receive_next(sid, &mut msg_opt).unwrap();
            let msg = msg_opt.as_mut().unwrap();
            let op = num_traits::FromPrimitive::from_usize(msg.body.id()).unwrap_or(GfxOpcode::InvalidCall);
            log::debug!("{:?}", op);
            match op {
                #[cfg(not(feature = "cramium-soc"))]
                GfxOpcode::SuspendResume => {
                    if let Some(scalar) = msg.body.scalar_message() {
                        let token = scalar.arg1;
                        display.suspend();
                        susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                        display.resume();
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::DrawClipObject => {
                    minigfx::handlers::draw_clip_object(&mut display, msg);
                }
                GfxOpcode::DrawClipObjectList => {
                    minigfx::handlers::draw_clip_object_list(&mut display, msg);
                }
                GfxOpcode::DrawTextView => {
                    minigfx::handlers::draw_text_view(&mut display, msg);
                }
                GfxOpcode::Flush => {
                    log::trace!("***gfx flush*** redraw##");
                    display.redraw();
                }
                GfxOpcode::Clear => {
                    let mut r = Rectangle::full_screen();
                    r.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 0);
                    op::rectangle(&mut display, r, screen_clip.into(), false)
                }
                GfxOpcode::Line => {
                    minigfx::handlers::line(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::Rectangle => {
                    minigfx::handlers::rectangle(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::RoundedRectangle => {
                    minigfx::handlers::rounded_rectangle(&mut display, screen_clip.into(), msg);
                }
                #[cfg(feature = "ditherpunk")]
                GfxOpcode::Tile => {
                    minigfx::handlers::tile(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::Circle => {
                    minigfx::handlers::circle(&mut display, screen_clip.into(), msg);
                }
                GfxOpcode::ScreenSize => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        let pt = display.screen_size();
                        scalar.arg1 = pt.x as usize;
                        scalar.arg2 = pt.y as usize;
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::QueryGlyphProps => {
                    minigfx::handlers::query_glyph_props(msg);
                }
                GfxOpcode::DrawSleepScreen => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        display.blit_screen(&logo::LOGO_MAP);
                        display.redraw();
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::DrawBootLogo => {
                    if let Some(_scalar) = msg.body.scalar_message() {
                        display.blit_screen(&poweron::LOGO_MAP);
                        display.redraw();
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::Devboot => {
                    if let Some(scalar) = msg.body.scalar_message() {
                        let ena = scalar.arg1;
                        if ena != 0 {
                            display.set_devboot(true);
                        } else {
                            display.set_devboot(false);
                        }
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::RestartBulkRead => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        bulkread.from_offset = 0;
                        scalar.arg1 = 0;
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::BulkReadFonts => {
                    // this also needs to reflect in root-keys/src/implementation.rs @ sign_loader()
                    let fontlen = fontmap::FONT_TOTAL_LEN as u32
                        + 16  // minver
                        + 16  // current ver
                        + 8; // sig ver + len
                    let mut buf =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    //let mut bulkread = buf.as_flat::<BulkRead, _>().unwrap(); // try to skip the copy/init
                    // step by using a persistent structure Safety: `u8` contains no
                    // undefined values
                    let fontslice = unsafe { fontregion.as_slice::<u8>() };
                    assert!(fontlen <= fontslice.len() as u32);
                    if bulkread.from_offset >= fontlen {
                        log::error!("BulkReadFonts attempt to read out of bound on the font area; ignoring!");
                        continue;
                    }
                    let readlen = if bulkread.from_offset + bulkread.buf.len() as u32 > fontlen {
                        // returns what is readable of the last bit; anything longer than the fontlen is
                        // undefined/invalid
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
                GfxOpcode::TestPattern => {
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        let duration = scalar.arg1;
                        let mut stashmem = xous::syscall::map_memory(
                            None,
                            None,
                            ((FB_SIZE * 4) + 4096) & !4095,
                            xous::MemoryFlags::R | xous::MemoryFlags::W,
                        )
                        .expect("couldn't map stash frame buffer");
                        // Safety: `u8` contains no undefined values
                        let stash = unsafe { &mut stashmem.as_slice_mut()[..FB_SIZE] };
                        for (&src, dst) in display.as_slice().iter().zip(stash.iter_mut()) {
                            *dst = src;
                        }
                        for lines in 0..FB_LINES {
                            // mark all lines dirty
                            stash[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                        }

                        let start_time = ticktimer.elapsed_ms();
                        let mut testmem = xous::syscall::map_memory(
                            None,
                            None,
                            ((FB_SIZE * 4) + 4096) & !4095,
                            xous::MemoryFlags::R | xous::MemoryFlags::W,
                        )
                        .expect("couldn't map stash frame buffer");
                        // Safety: `u8` contains no undefined values
                        let testpat = unsafe { &mut testmem.as_slice_mut()[..FB_SIZE] };
                        const DWELL: usize = 1000;
                        while ticktimer.elapsed_ms() - start_time < duration as u64 {
                            // all black
                            for w in testpat.iter_mut() {
                                *w = 0;
                            }
                            for lines in 0..FB_LINES {
                                // mark dirty bits
                                testpat[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
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
                            for lines in 0..FB_LINES {
                                for words in 0..FB_WIDTH_WORDS {
                                    testpat[lines * FB_WIDTH_WORDS + words] = 0xaaaa_aaaa;
                                }
                            }
                            display.blit_screen(testpat);
                            display.redraw();
                            ticktimer.sleep_ms(DWELL).unwrap();

                            for lines in 0..FB_LINES {
                                for words in 0..FB_WIDTH_WORDS {
                                    testpat[lines * FB_WIDTH_WORDS + words] = 0x5555_5555;
                                }
                            }
                            display.blit_screen(testpat);
                            display.redraw();
                            ticktimer.sleep_ms(DWELL).unwrap();

                            // horiz bars
                            for lines in 0..FB_LINES {
                                for words in 0..FB_WIDTH_WORDS {
                                    if lines % 2 == 0 {
                                        testpat[lines * FB_WIDTH_WORDS + words] = 0x0;
                                    } else {
                                        testpat[lines * FB_WIDTH_WORDS + words] = 0xffff_ffff;
                                    }
                                }
                                testpat[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                            }
                            display.blit_screen(testpat);
                            display.redraw();
                            ticktimer.sleep_ms(DWELL).unwrap();

                            for lines in 0..FB_LINES {
                                for words in 0..FB_WIDTH_WORDS {
                                    if lines % 2 == 1 {
                                        testpat[lines * FB_WIDTH_WORDS + words] = 0x0;
                                    } else {
                                        testpat[lines * FB_WIDTH_WORDS + words] = 0xffff_ffff;
                                    }
                                }
                                testpat[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
                            }
                            display.blit_screen(testpat);
                            display.redraw();
                            ticktimer.sleep_ms(DWELL).unwrap();
                        }
                        display.blit_screen(stash);

                        scalar.arg1 = duration;
                    } else {
                        panic!("Incorrect message type");
                    }
                }
                GfxOpcode::Stash => {
                    display.stash();
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                    // no failure if it's not
                }
                GfxOpcode::Pop => {
                    display.pop();
                    if let Some(scalar) = msg.body.scalar_message_mut() {
                        // ack the message if it's a blocking scalar
                        scalar.arg1 = 1;
                    }
                    // no failure if it's not
                }
                GfxOpcode::AcquireModal | GfxOpcode::ReleaseModal => {
                    unimplemented!(
                        "Acquire/Release modal is not supported for this target. Use the GAM instead."
                    );
                }
                GfxOpcode::Quit => break,
                GfxOpcode::InvalidCall => {
                    log::error!("Received invalid GfxOpcode. Ignoring.");
                }
                _ => {
                    // This is perfectly normal because not all opcodes are handled by all platforms.
                    log::debug!("Invalid or unhandled opcode: {:?}", op);
                }
            }
        } else {
            // this is effectively an abort, because this is long enough for the WDT to fire and reboot the
            // system
            ticktimer.sleep_ms(10_000).unwrap();
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(sid).unwrap();
    xous::destroy_server(sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
