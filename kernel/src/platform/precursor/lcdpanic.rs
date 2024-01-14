use utralib::generated::*;
use xous_kernel::PID;

use crate::mem::MemoryManager;

const FB_WIDTH_WORDS: usize = 11;
const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
// this font is from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
const FONT_IMAGE: &'static [u8] = include_bytes!("font6x12_1bpp.raw");
const CHAR_HEIGHT: u32 = 12;
const CHAR_WIDTH: u32 = 6;
const FONT_IMAGE_WIDTH: u32 = 96;
const LEFT_MARGIN: i16 = 2;

const LCD_CONTROL_VIRT: usize = 0xffcb_0000;
const LCD_FB_VIRT: usize = 0xffca_0000;

#[derive(Clone, Copy)]
struct Point {
    x: i16,
    y: i16,
}

pub struct ErrorWriter {
    gfx: Gfx,
    point: Point,
}

impl ErrorWriter {
    pub fn new() -> Result<ErrorWriter, &'static str> {
        let gfx = Gfx::new()?;
        let point = Point { x: LEFT_MARGIN, y: 15 };
        Ok(ErrorWriter { gfx, point })
    }
}

impl core::fmt::Write for ErrorWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut point = self.point;
        self.gfx.msg(s, &mut point);
        self.point = point;
        Ok(())
    }
}

#[derive(PartialEq, Eq)]
enum Color {
    Light,
    Dark,
}

struct Gfx {
    csr: utralib::CSR<u32>,
    fb: &'static mut [u32],
}

impl Gfx {
    pub fn new() -> Result<Gfx, &'static str> {
        // Steal the LCD from userspace
        if MemoryManager::with_mut(|memory_manager| {
            crate::arch::mem::map_page_inner(
                memory_manager,
                PID::new(1).unwrap(),
                HW_MEMLCD_BASE as usize,
                LCD_CONTROL_VIRT,
                xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
                false,
            )
        })
        .is_err()
        {
            return Err("unable to map LCD");
        }

        // Steal the LCD from userspace
        for i in (0..(FB_WIDTH_WORDS * FB_LINES * 4 + 4096)).step_by(4096) {
            if MemoryManager::with_mut(|memory_manager| {
                crate::arch::mem::map_page_inner(
                    memory_manager,
                    PID::new(1).unwrap(),
                    HW_MEMLCD_MEM as usize + i,
                    LCD_FB_VIRT + i,
                    xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
                    false,
                )
            })
            .is_err()
            {
                return Err("unable to map LCD");
            }
        }
        let mut gfx = Gfx {
            csr: CSR::new(LCD_CONTROL_VIRT as *mut u32),
            fb: unsafe { core::slice::from_raw_parts_mut(LCD_FB_VIRT as *mut u32, FB_SIZE) },
        };

        gfx.init(100_000_000);

        for (i, word) in gfx.fb.iter_mut().enumerate() {
            if i % 2 == 0 {
                *word = 0xAAAA_AAAA;
            } else {
                *word = 0x5555_5555;
            }
        }
        Ok(gfx)
    }

    pub fn init(&mut self, clk_mhz: u32) {
        self.csr.wfo(utra::memlcd::PRESCALER_PRESCALER, (clk_mhz / 2_000_000) - 1);
    }

    #[allow(dead_code)]
    pub fn update_all(&mut self) { self.csr.wfo(utra::memlcd::COMMAND_UPDATEALL, 1); }

    pub fn update_dirty(&mut self) { self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1); }

    pub fn busy(&self) -> bool { self.csr.rf(utra::memlcd::BUSY_BUSY) == 1 }

    pub fn flush(&mut self) {
        self.update_dirty();
        while self.busy() {}
        // clear the dirty bits
        for lines in 0..FB_LINES {
            self.fb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
        }
    }

    fn char_offset(&self, c: u8) -> u32 {
        let fallback = b' ' as u32 - b' ' as u32;
        if c < b' ' {
            return fallback;
        }
        if c <= b'~' {
            return c as u32 - b' ' as u32;
        }
        fallback
    }

    pub fn msg(&mut self, text: &str, pos: &mut Point) {
        // this routine is adapted from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
        let char_per_row = FONT_IMAGE_WIDTH / CHAR_WIDTH;
        for current_char in text.as_bytes() {
            let mut char_walk_x = 0;
            let mut char_walk_y = 0;

            // See if we need to wrap. We do this at the top in order to avoid
            // inserting an extra lf if the line is exactly as wide as the screen.
            if *current_char != b'\n'
                && *current_char != b'\r'
                && pos.x + (3 * CHAR_WIDTH as i16) >= FB_WIDTH_PIXELS as i16 - LEFT_MARGIN
            {
                // Line wrapping
                pos.x = LEFT_MARGIN as i16;
                pos.y += CHAR_HEIGHT as i16;
            }

            loop {
                // Char _code_ offset from first char, most often a space
                // E.g. first char = ' ' (32), target char = '!' (33), offset = 33 - 32 = 1
                let char_offset = self.char_offset(*current_char);
                let row = char_offset / char_per_row;

                // Top left corner of character, in pixels
                let char_x = (char_offset - (row * char_per_row)) * CHAR_WIDTH;
                let char_y = row * CHAR_HEIGHT;

                // Bit index
                // = X pixel offset for char
                // + Character row offset (row 0 = 0, row 1 = (192 * 8) = 1536)
                // + X offset for the pixel block that comprises this char
                // + Y offset for pixel block
                let bitmap_bit_index =
                    char_x + (FONT_IMAGE_WIDTH * char_y) + char_walk_x + (char_walk_y * FONT_IMAGE_WIDTH);

                let bitmap_byte = bitmap_bit_index / 8;
                let bitmap_bit = 7 - (bitmap_bit_index % 8);

                let color = if FONT_IMAGE[bitmap_byte as usize] & (1 << bitmap_bit) != 0 {
                    Color::Light
                } else {
                    Color::Dark
                };

                let x = pos.x + CHAR_WIDTH as i16 + char_walk_x as i16;
                let y = pos.y + char_walk_y as i16;

                // draw color at x, y
                if (*current_char != b'\r') && (*current_char != b'\n') {
                    // don't draw CRLF specials
                    self.draw_pixel(Point { x, y }, color);
                }

                char_walk_x += 1;

                if char_walk_x >= CHAR_WIDTH {
                    char_walk_x = 0;
                    char_walk_y += 1;

                    // Done with this char, move on to the next one
                    if char_walk_y >= CHAR_HEIGHT {
                        if *current_char == b'\n' {
                            // '\n'
                            pos.x = LEFT_MARGIN as i16;
                            pos.y += CHAR_HEIGHT as i16;
                        } else if *current_char == b'\r' {
                            // '\r'
                        } else {
                            pos.x += CHAR_WIDTH as i16;
                        }

                        break;
                    }
                }
            }
        }
        self.flush();
    }

    pub fn draw_pixel(&mut self, pix: Point, color: Color) {
        let mut clip_y: usize = pix.y as usize;
        if clip_y >= FB_LINES {
            clip_y = FB_LINES - 1;
        }
        let clip_x: usize = pix.x as usize;
        if clip_x >= FB_WIDTH_PIXELS {
            clip_y = FB_WIDTH_PIXELS - 1;
        }
        if color == Color::Light {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] |= 1 << (clip_x % 32)
        } else {
            self.fb[(clip_x + clip_y * FB_WIDTH_WORDS * 32) / 32] &= !(1 << (clip_x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        self.fb[clip_y * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
    }
}
