use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use bao1x_api::{EventChannel, IoxHal};
use bao1x_hal::ifram::IframRange;
use bao1x_hal::sh1107::{Mono, Oled128x128};
use bao1x_hal::udma;
use bao1x_hal::udma::CommandSet;
use blitstr2::ClipRect;
use blitstr2::NULL_GLYPH_SPRITE;
/// THIS NEEDS SUBSTANTIAL REWORK, THE DRIVER HERE TARGETS A DIFFERENT DISPLAY THAN THE SH1107.

/// We can have no allocations inside this, and ideally, it's as minimal as possible.
///
/// The graphics handler code and font renderer code are duplicated into this module to
/// minimize cross-thread dependencies.
///
/// Some simplifying assumptions include:
///   - Only Latin, monospace (16-px wide) characters from the `mono` font set.
///   - Panics occupy a fixed area in the center of the screen, that can only hold a limited amount of
///     text: 304-px x 384-px text area = 40 char x 24 lines = 960 chars
///   - The Panic frame is slightly larger to give a cosmetic border
///   - Panics are black background with white text to distinguish them as secured system messages.
///   - Panics can't be dismissed, and should persist even if other threads are capable of running.
///   - The Panic handler can conflict with the regular display routines because it unsafely creates a
///     copy of all the hardware access structures. Thus there is a variable shared with the parent
///     thread to stop redraws permanently in the case of a panic.
///
/// Note that the frame buffer is 336 px wide, which is 10.5 32-bit words.
/// The excess 16 bits are the dirty bit field.
use ux_api::minigfx::*;

pub const PANIC_STD_SERVER: &'static str = "panic-to-screen!";

/// these are fixed by the monospace font
const GLYPH_HEIGHT: isize = 15;
const GLYPH_WIDTH: isize = 7;
/// this can be adjusted to create more border around the panic box
const TEXT_MARGIN: isize = 0;

pub(crate) fn panic_handler_thread(
    is_panic: Arc<AtomicBool>,
    display_parts: (
        usize,
        udma::SpimCs,
        u8,
        u8,
        Option<EventChannel>,
        udma::SpimMode,
        udma::SpimByteAlign,
        IframRange,
        usize,
        usize,
        u8,
        Option<CommandSet>,
    ),
) {
    thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let panic_server = xns.register_name(crate::panic::PANIC_STD_SERVER, None).unwrap();
            let iox_panic = IoxHal::new();
            let mut display = unsafe { Oled128x128::from_raw_parts(display_parts, &iox_panic) };

            // current x/y position of the latest character to add to the panic box
            let mut x = TEXT_MARGIN;
            let mut y = TEXT_MARGIN;
            let cr = ClipRect::full_screen();

            let mut append_string = |s: &str, d: &mut Oled128x128| {
                for ch in s.chars() {
                    if ch == '\n' {
                        y += GLYPH_HEIGHT;
                        x = TEXT_MARGIN;
                        continue;
                    }
                    blitstr2::xor_glyph(
                        unsafe { d.raw_mut() },
                        (x, y),
                        &blitstr2::mono_glyph(ch).unwrap_or(NULL_GLYPH_SPRITE),
                        true,
                        cr,
                    );
                    x += GLYPH_WIDTH;
                    if x > 128 {
                        x = TEXT_MARGIN;
                        y += GLYPH_HEIGHT;
                    }
                }
            };

            loop {
                let msg = match xous::receive_message(panic_server) {
                    Ok(m) => m,
                    Err(e) => panic!("Error in panic {:?}", e),
                };

                // this will put the graphics renderer in panic mode.
                if !is_panic.load(Ordering::Relaxed) {
                    // this locks out updates from the main loop
                    is_panic.store(true, Ordering::Relaxed);

                    // draw the "panic rectangle"
                    for y in 0..128 {
                        for x in 0..128 {
                            display.put_pixel(Point::new(x, y), Mono::White.into());
                        }
                    }

                    display.draw();
                    append_string("~Guru Meditation~\n", &mut display);
                }
                let body = match msg.body.memory_message() {
                    Some(body) => body,
                    None => {
                        log::error!("Incorrect message type to panic renderer");
                        return;
                    }
                };
                let len = match body.valid {
                    Some(v) => v,
                    None => continue, // ignore, don't fail
                }
                .get();
                let s = unsafe { core::str::from_utf8_unchecked(&body.buf.as_slice()[..len]) };
                append_string(s, &mut display);
                display.draw();
            }
        }
    });
}
