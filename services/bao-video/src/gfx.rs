#[cfg(feature = "hosted-baosec")]
use bao1x_emu::display::Mono;
#[cfg(feature = "board-baosec")]
use bao1x_hal::sh1107::Mono;
use ux_api::minigfx::*;

// The discipline for all the APIs in this module are that they act on a FrameBuffer which is
// passed to the function. This allows us to bind the drawing computation on the caller-side of
// of a shared frame buffer.

// this font is from the embedded graphics crate https://docs.rs/embedded-graphics/0.7.1/embedded_graphics/
const FONT_IMAGE: &'static [u8] = include_bytes!("../../../loader/src/font6x12_1bpp.raw");
pub const CHAR_HEIGHT: isize = 12;
pub const CHAR_WIDTH: isize = 6;
const FONT_IMAGE_WIDTH: isize = 96;
const LEFT_MARGIN: isize = 10;

fn char_offset(c: char) -> isize {
    let fallback = ' ' as isize - ' ' as isize;
    if c < ' ' {
        return fallback;
    }
    if c <= '~' {
        return c as isize - ' ' as isize;
    }
    fallback
}

pub fn msg<'a>(
    fb: &mut dyn FrameBuffer,
    text: &'a str,
    ll_pos: Point,
    fg: ColorNative,
    bg: ColorNative,
    upside_down: bool,
    screen_size: Point, // e.g. Point::new(SCREEN_WIDTH, SCREEN_HEIGHT); ignored when !upside_down
) {
    let mut ll_pos = ll_pos; // no need for .clone() on Copy types
    let char_per_row = FONT_IMAGE_WIDTH / CHAR_WIDTH;
    let mut idx: isize = 0;

    for current_char in text.chars() {
        let cur_byte = current_char as u8;
        let is_cr = cur_byte == 0x0d;
        let is_lf = cur_byte == 0x0a;
        let is_visible = !is_cr && !is_lf;

        // -- Per-character constants (hoisted out of the pixel loop) ----------
        let char_offset = char_offset(current_char);
        let row = char_offset / char_per_row;
        let char_x = (char_offset - (row * char_per_row)) * CHAR_WIDTH;
        let char_y = row * CHAR_HEIGHT;
        // Base bit index in the font bitmap for the top-left of this glyph
        let base_bitmap_idx = char_x + FONT_IMAGE_WIDTH * char_y;
        // Screen X of the left edge of this character
        let char_screen_x = ll_pos.x + CHAR_WIDTH * idx;

        // -- Pixel walk -------------------------------------------------------
        let mut char_walk_x: isize = 0;
        let mut char_walk_y: isize = 0;
        loop {
            if is_visible {
                let bitmap_bit_idx = base_bitmap_idx + char_walk_x + char_walk_y * FONT_IMAGE_WIDTH;
                let bitmap_byte = (bitmap_bit_idx / 8) as usize;
                let bitmap_bit = bitmap_bit_idx as u8 & 7 ^ 7;
                let color = if FONT_IMAGE[bitmap_byte] & (1 << bitmap_bit) != 0 { fg } else { bg };

                let px = char_screen_x + char_walk_x;
                let py = ll_pos.y + char_walk_y;

                // For upside-down: mirror the screen position.
                let draw_point = if upside_down {
                    Point::new(screen_size.x - 1 - px, screen_size.y - 1 - py)
                } else {
                    Point::new(px, py)
                };

                fb.put_pixel(draw_point, color);
            }

            char_walk_x += 1;
            if char_walk_x >= CHAR_WIDTH {
                char_walk_x = 0;
                char_walk_y += 1;
                if char_walk_y >= CHAR_HEIGHT {
                    if is_cr {
                        ll_pos.y += CHAR_HEIGHT;
                    } else if is_lf {
                        ll_pos.x = LEFT_MARGIN;
                        idx = 0;
                    } else {
                        idx += 1;
                    }
                    break;
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn line(fb: &mut dyn FrameBuffer, l: Line, clip: Option<Rectangle>, xor: bool) {
    let color: ColorNative;
    if l.style.stroke_color.is_some() {
        color = l.style.stroke_color.unwrap().into();
    } else {
        return;
    }
    let mut x0 = l.start.x;
    let mut y0 = l.start.y;
    let x1 = l.end.x;
    let y1 = l.end.y;

    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -((y1 - y0).abs());
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy; // error value e_xy
    loop {
        if x0 >= 0 && y0 >= 0 && x0 < (fb.dimensions().x as _) && y0 < (fb.dimensions().y as _) {
            if clip.is_none() || (clip.unwrap().intersects_point(Point::new(x0, y0))) {
                if !xor {
                    fb.put_pixel(Point::new(x0 as _, y0 as _), color);
                } else {
                    if let Some(existing) = fb.get_pixel(Point::new(x0 as _, y0 as _)) {
                        if existing == Mono::Black.into() {
                            fb.put_pixel(Point::new(x0 as _, y0 as _), Mono::White.into());
                        } else {
                            fb.put_pixel(Point::new(x0 as _, y0 as _), Mono::Black.into());
                        }
                    }
                }
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}
