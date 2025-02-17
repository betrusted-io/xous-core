use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use blitstr2::{GlyphSprite, NULL_GLYPH_SPRITE};
use utralib::generated::*;

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
use crate::{
    api::PixelColor,
    backend::{FB_SIZE, FB_WIDTH_PIXELS, FB_WIDTH_WORDS},
};
/// How far down the screen the panic box draws
const TOP_OFFSET: usize = 48;
/// Width and height of the panic box in characters
const WIDTH_CHARS: usize = 40;
const HEIGHT_CHARS: usize = 24;
/// these are fixed by the monospace font
const GLYPH_HEIGHT: usize = 15;
const GLYPH_WIDTH: usize = 7;
/// this can be adjusted to create more border around the panic box
const TEXT_MARGIN: usize = 8;

/// some derived constants to help with layout
const BOTTOM_LINE: usize = TOP_OFFSET + HEIGHT_CHARS * GLYPH_HEIGHT + TEXT_MARGIN * 2;
const LEFT_EDGE: usize = (FB_WIDTH_PIXELS - (WIDTH_CHARS * GLYPH_WIDTH + TEXT_MARGIN * 2)) / 2; // 24
const RIGHT_EDGE: usize = FB_WIDTH_PIXELS - LEFT_EDGE; // 312

pub(crate) fn panic_handler_thread(is_panic: Arc<AtomicBool>, hwfb: u32, control: u32) {
    thread::spawn({
        move || {
            let xns = xous_names::XousNames::new().unwrap();
            let panic_server = xns.register_name(crate::panic::PANIC_STD_SERVER, None).unwrap();

            let mut display = unsafe { PanicDisplay::new(hwfb, control) };
            loop {
                let msg = xous::receive_message(panic_server).unwrap();

                // this will put the graphics renderer in panic mode.
                if !is_panic.load(Ordering::Relaxed) {
                    // this locks out updates from the main loop
                    is_panic.store(true, Ordering::Relaxed);
                    display.panic_rectangle();
                    display.append_string("          ~~Guru Meditation~~\n\n");
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
                display.append_string(s);
                display.update_dirty();
            }
        }
    });
}

/// All-in-one object to manage a framebuffer and layout text.
struct PanicDisplay<'a> {
    /// hardware framebuffer (no double buffering)
    fb: &'a mut [u32],
    /// hardware register copy (very dangerous)
    csr: CSR<u32>,
    /// current x/y position of the latest character to add to the panic box
    x: usize,
    y: usize,
}
impl<'a> PanicDisplay<'a> {
    /// # Safety
    ///
    /// This function takes "plain old 32-bit numbers" and transforms them
    /// into hardware pointers to the frame buffer and CSR offsets. It's meant
    /// to be pared with the `hw_regs` output from the XousDisplay object, and
    /// there will be frame buffer conflicts unless some kind of mutex is wrapped
    /// around any other code that might compete for access to these resources.
    unsafe fn new(raw_fb: u32, control: u32) -> Self {
        PanicDisplay {
            fb: core::slice::from_raw_parts_mut(raw_fb as *mut u32, FB_SIZE),
            csr: CSR::new(control as *mut u32),
            // initialize to top left of panic box's margin
            x: TEXT_MARGIN,
            y: TEXT_MARGIN,
        }
    }

    /// commits the data in the framebuffer to the screen
    fn update_dirty(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1);
        while self.busy() {}
    }

    /// indicates if a commit is in progress
    fn busy(&self) -> bool { self.csr.rf(utra::memlcd::BUSY_BUSY) == 1 }

    fn put_pixel(&mut self, x: usize, y: usize, color: PixelColor) {
        if color == PixelColor::Light {
            self.fb[(x + y * FB_WIDTH_WORDS * 32) / 32] |= 1 << (x % 32)
        } else {
            self.fb[(x + y * FB_WIDTH_WORDS * 32) / 32] &= !(1 << (x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        self.fb[y * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
    }

    /// draw a black rectangle into which all the characters are placed. This could
    /// probably be optimized to do its work in words instead of as pixels, but it works.
    fn panic_rectangle(&mut self) {
        for y in TOP_OFFSET..BOTTOM_LINE {
            for x in LEFT_EDGE..RIGHT_EDGE {
                self.put_pixel(x, y, PixelColor::Dark);
            }
        }
    }

    /// Blit a glyph
    /// Examples of word alignment for destination frame buffer:
    /// 1. Fits in word: xr:1..7   => (data[0].bit_30)->(data[0].bit_26), mask:0x7c00_0000
    /// 2. Spans words:  xr:30..36 => (data[0].bit_01)->(data[1].bit_29), mask:[0x0000_0003,0xe000_000]
    ///
    /// This is copied out of the blitstr2 module and adapted for the panic handler.
    fn draw_glyph(&mut self, x: usize, y: usize, gs: GlyphSprite) {
        const SPRITE_PX: i16 = 16;
        const SPRITE_WORDS: i16 = 8;
        if gs.glyph.len() < SPRITE_WORDS as usize {
            // Fail silently if the glyph slice was too small
            // TODO: Maybe return an error? Not sure which way is better.
            return;
        }
        let high = gs.high as i16;
        let wide = gs.wide as i16;
        if high > SPRITE_PX || wide > SPRITE_PX {
            // Fail silently if glyph height or width is out of spec
            // TODO: Maybe return an error?
            return;
        }
        // Calculate word alignment for destination buffer
        let x0 = (x + LEFT_EDGE) as i16;
        let x1 = (x + LEFT_EDGE) as i16 + wide - 1;
        let dest_low_word = x0 >> 5;
        let dest_high_word = x1 >> 5;
        let px_in_dest_low_word = 32 - (x0 & 0x1f);
        // Blit it (use glyph height to avoid blitting empty rows)
        let mut row_base = (y + TOP_OFFSET) as i16 * FB_WIDTH_WORDS as i16;
        let row_upper_limit = BOTTOM_LINE as i16 * FB_WIDTH_WORDS as i16;
        let row_lower_limit = TOP_OFFSET as i16 * FB_WIDTH_WORDS as i16;
        let glyph = gs.glyph;
        for y in 0..high as usize {
            if row_base >= row_upper_limit {
                // Clip anything that would run off the end of the frame buffer
                break;
            }
            if row_base >= row_lower_limit {
                // Unpack pixels for this glyph row.
                // CAUTION: some math magic happening here...
                //  when y==0, this does (glyph[0] >>  0) & mask,
                //  when y==1, this does (glyph[0] >> 16) & mask,
                //  when y==2, this does (glyph[1] >>  0) & mask,
                //  ...
                let mask = 0x0000ffff as u32;
                let shift = (y as u32 & 1) << 4;
                let pattern = (glyph[y >> 1] >> shift) & mask;

                // compute partial masks to prevent glyphs from "spilling over" the clip rectangle
                let mut partial_mask_lo = 0xffff_ffff;
                let mut partial_mask_hi = 0xffff_ffff;
                if x0 < LEFT_EDGE as i16 || x1 >= RIGHT_EDGE as i16 {
                    let x0a = if x0 < LEFT_EDGE as i16 { RIGHT_EDGE as i16 } else { x0 };
                    let x1a = if x1 >= LEFT_EDGE as i16 { RIGHT_EDGE as i16 } else { x1 };
                    let mut ones = (1u64 << ((x1a - x0a) as u64 + 1)) - 1;
                    ones <<= x0a as u64 & 0x1f;
                    partial_mask_lo = ones as u32;
                    partial_mask_hi = (ones >> 32) as u32;
                }
                // XOR glyph pixels onto destination buffer. Note that despite the masks above, we will not
                // render partial glyphs that cross the absolute bounds of the left and right
                // edge of the screen.
                if x0 >= 0 && x1 < FB_WIDTH_PIXELS as i16 {
                    self.fb[(row_base + dest_low_word) as usize] ^=
                        (pattern << (32 - px_in_dest_low_word)) & partial_mask_lo;
                    if wide > px_in_dest_low_word {
                        self.fb[(row_base + dest_high_word) as usize] ^=
                            (pattern >> px_in_dest_low_word) & partial_mask_hi;
                    }
                }
                self.fb[(row_base as usize + FB_WIDTH_WORDS - 1) as usize] |= 0x1_0000; // set the dirty bit on the line
            }

            // Advance destination offset using + instead of * to maybe save some CPU cycles
            row_base += FB_WIDTH_WORDS as i16;
        }
    }

    /// start laying out the string from the top left and just wrap character by character, considering
    /// newlines
    fn append_string(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.y += GLYPH_HEIGHT;
                self.x = TEXT_MARGIN;
                continue;
            }
            self.draw_glyph(self.x, self.y, blitstr2::mono_glyph(ch).unwrap_or(NULL_GLYPH_SPRITE));
            self.x += GLYPH_WIDTH;
            if self.x >= WIDTH_CHARS * GLYPH_WIDTH {
                self.x = TEXT_MARGIN;
                self.y += GLYPH_HEIGHT;
            }
        }
    }
}
