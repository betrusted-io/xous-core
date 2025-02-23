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
use num_traits::FromPrimitive;
use xous::{MemoryRange, msg_blocking_scalar_unpack, msg_scalar_unpack};
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
fn map_fonts() -> MemoryRange {
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
    loop {
        if !is_panic.load(Ordering::Relaxed) {
            // non-panic graphics operations if we are in a panic situation
            let mut msg = xous::receive_message(sid).unwrap();
            let op = FromPrimitive::from_usize(msg.body.id());
            log::trace!("{:?}", op);
            match op {
                #[cfg(not(feature = "cramium-soc"))]
                Some(GfxOpcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                    display.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    display.resume();
                }),
                Some(GfxOpcode::DrawClipObject) => {
                    minigfx::handlers::draw_clip_object(&mut display, &mut msg);
                }
                Some(GfxOpcode::DrawClipObjectList) => {
                    minigfx::handlers::draw_clip_object_list(&mut display, &mut msg);
                }
                Some(GfxOpcode::DrawTextView) => {
                    minigfx::handlers::draw_text_view(&mut display, &mut msg);
                }
                Some(GfxOpcode::Flush) => {
                    log::trace!("***gfx flush*** redraw##");
                    display.redraw();
                }
                Some(GfxOpcode::Clear) => {
                    let mut r = Rectangle::full_screen();
                    r.style = DrawStyle::new(PixelColor::Light, PixelColor::Light, 0);
                    op::rectangle(&mut display, r, screen_clip.into(), false)
                }
                Some(GfxOpcode::Line) => {
                    minigfx::handlers::line(&mut display, screen_clip.into(), &mut msg);
                }
                Some(GfxOpcode::Rectangle) => {
                    minigfx::handlers::rectangle(&mut display, screen_clip.into(), &mut msg);
                }
                Some(GfxOpcode::RoundedRectangle) => {
                    minigfx::handlers::rounded_rectangle(&mut display, screen_clip.into(), &mut msg);
                }
                #[cfg(feature = "ditherpunk")]
                Some(GfxOpcode::Tile) => {
                    minigfx::handlers::tile(&mut display, screen_clip.into(), &mut msg);
                }
                Some(GfxOpcode::Circle) => {
                    minigfx::handlers::circle(&mut display, screen_clip.into(), &mut msg);
                }
                Some(GfxOpcode::ScreenSize) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    let pt = display.screen_size();
                    xous::return_scalar2(msg.sender, pt.x as usize, pt.y as usize)
                        .expect("couldn't return ScreenSize request");
                }),
                Some(GfxOpcode::QueryGlyphProps) => {
                    minigfx::handlers::query_glyph_props(&mut msg);
                }
                Some(GfxOpcode::DrawSleepScreen) => msg_scalar_unpack!(msg, _, _, _, _, {
                    display.blit_screen(&logo::LOGO_MAP);
                    display.redraw();
                }),
                Some(GfxOpcode::DrawBootLogo) => msg_scalar_unpack!(msg, _, _, _, _, {
                    display.blit_screen(&poweron::LOGO_MAP);
                    display.redraw();
                }),
                Some(GfxOpcode::Devboot) => msg_scalar_unpack!(msg, ena, _, _, _, {
                    if ena != 0 {
                        display.set_devboot(true);
                    } else {
                        display.set_devboot(false);
                    }
                }),
                Some(GfxOpcode::RestartBulkRead) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    bulkread.from_offset = 0;
                    xous::return_scalar(msg.sender, 0)
                        .expect("couldn't ack that bulk read pointer was reset");
                }),
                Some(GfxOpcode::BulkReadFonts) => {
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
                Some(GfxOpcode::TestPattern) => msg_blocking_scalar_unpack!(msg, duration, _, _, _, {
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

                    xous::return_scalar(msg.sender, duration).expect("couldn't ack test pattern");
                }),
                Some(GfxOpcode::Stash) => {
                    display.stash();
                    match msg.body {
                        // ack the message if it's a blocking scalar
                        xous::Message::BlockingScalar(_) => xous::return_scalar(msg.sender, 1).unwrap(),
                        _ => (),
                    }
                }
                Some(GfxOpcode::Pop) => {
                    display.pop();
                    match msg.body {
                        // ack the message if it's a blocking scalar
                        xous::Message::BlockingScalar(_) => xous::return_scalar(msg.sender, 1).unwrap(),
                        _ => (),
                    }
                }
                Some(GfxOpcode::Quit) => break,
                None => {
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
