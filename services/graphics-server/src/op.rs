use crate::api::{Circle, DrawStyle, Line, Pixel, PixelColor, Point, Rectangle};

/// LCD Frame buffer bounds
pub const LCD_WORDS_PER_LINE: usize = 11;
pub const LCD_PX_PER_LINE: usize = 336;
pub const LCD_LINES: usize = 536;
pub const LCD_FRAME_BUF_SIZE: usize = LCD_WORDS_PER_LINE * LCD_LINES;

pub const WIDTH: i16 = 336;
pub const HEIGHT: i16 = 536;

/// For passing frame buffer references
pub type LcdFB = [u32; LCD_FRAME_BUF_SIZE];

/// For storing a full-row wide blit pattern
pub type BlitRow = [u32; LCD_WORDS_PER_LINE];

fn put_pixel(fb: &mut LcdFB, x: i16, y: i16, color: PixelColor) {
    let mut clip_y: usize = y as usize;
    if clip_y >= LCD_LINES {
        clip_y = LCD_LINES - 1;
    }

    let clip_x: usize = x as usize;
    if clip_x >= LCD_PX_PER_LINE {
        clip_y = LCD_PX_PER_LINE - 1;
    }

    if color == PixelColor::Dark {
        fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] |= 1 << (clip_x % 32)
    } else {
        fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] &= !(1 << (clip_x % 32))
    }
    // set the dirty bit on the line that contains the pixel
    fb[clip_y * LCD_WORDS_PER_LINE + (LCD_WORDS_PER_LINE - 1)] |= 0x1_0000;
}

pub fn line(fb: &mut LcdFB, l: Line) {
    let color: PixelColor;
    if l.style.stroke_color.is_some() {
        color = l.style.stroke_color.unwrap();
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
    let mut err = dx + dy; /* error value e_xy */
    loop {
        /* loop */
        if x0 >= 0 && y0 >= 0 && x0 < (WIDTH as _) && y0 < (HEIGHT as _) {
            put_pixel(fb, x0 as _, y0 as _, color);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            /* e_xy+e_x > 0 */
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            /* e_xy+e_y < 0 */
            err += dx;
            y0 += sy;
        }
    }
}

/// Pixel iterator for each pixel in the circle border
/// lifted from embedded-graphics crate
#[derive(Debug, Copy, Clone)]
pub struct CircleIterator {
    center: Point,
    radius: u16,
    style: DrawStyle,
    p: Point,
}

impl Iterator for CircleIterator {
    type Item = Pixel;

    // https://stackoverflow.com/questions/1201200/fast-algorithm-for-drawing-filled-circles
    fn next(&mut self) -> Option<Self::Item> {
        // If border or stroke colour is `None`, treat entire object as transparent and exit early
        if self.style.stroke_color.is_none() && self.style.fill_color.is_none() {
            return None;
        }

        let radius = self.radius as i16 - self.style.stroke_width + 1;
        let outer_radius = self.radius as i16;

        let radius_sq = radius * radius;
        let outer_radius_sq = outer_radius * outer_radius;

        loop {
            let t = self.p;
            let len = t.x * t.x + t.y * t.y;

            let is_border = len > radius_sq - radius && len < outer_radius_sq + radius;

            let is_fill = len <= outer_radius_sq + 1;

            let item = if is_border && self.style.stroke_color.is_some() {
                Some(Pixel(
                    self.center + t,
                    self.style.stroke_color.expect("Border color not defined"),
                ))
            } else if is_fill && self.style.fill_color.is_some() {
                Some(Pixel(
                    self.center + t,
                    self.style.fill_color.expect("Fill color not defined"),
                ))
            } else {
                None
            };

            self.p.x += 1;

            if self.p.x > self.radius as i16 {
                self.p.x = -(self.radius as i16);
                self.p.y += 1;
            }

            if self.p.y > self.radius as i16 {
                break None;
            }

            if item.is_some() {
                break item;
            }
        }
    }
}

pub fn circle(fb: &mut LcdFB, circle: Circle) {
    let c = CircleIterator {
        center: circle.center,
        radius: circle.radius as _,
        style: circle.style,
        p: Point::new(-(circle.radius as i16), -(circle.radius as i16)),
    };

    for pixel in c {
        put_pixel(fb, pixel.0.x, pixel.0.y, pixel.1);
    }
}

/// Pixel iterator for each pixel in the rect border
/// lifted from embedded-graphics crate
#[derive(Debug, Clone, Copy)]
pub struct RectangleIterator {
    top_left: Point,
    bottom_right: Point,
    style: DrawStyle,
    p: Point,
}

impl Iterator for RectangleIterator {
    type Item = Pixel;

    fn next(&mut self) -> Option<Self::Item> {
        // Don't render anything if the rectangle has no border or fill color.
        if self.style.stroke_color.is_none() && self.style.fill_color.is_none() {
            return None;
        }

        loop {
            let mut out = None;

            // Finished, i.e. we're below the rect
            if self.p.y > self.bottom_right.y {
                break None;
            }

            let border_width = self.style.stroke_width;
            let tl = self.top_left;
            let br = self.bottom_right;

            // Border
            if (
                // Top border
                (self.p.y >= tl.y && self.p.y < tl.y + border_width)
            // Bottom border
            || (self.p.y <= br.y && self.p.y > br.y - border_width)
            // Left border
            || (self.p.x >= tl.x && self.p.x < tl.x + border_width)
            // Right border
            || (self.p.x <= br.x && self.p.x > br.x - border_width)
            ) && self.style.stroke_color.is_some()
            {
                out = Some(Pixel(
                    self.p,
                    self.style.stroke_color.expect("Expected stroke"),
                ));
            }
            // Fill
            else if let Some(fill) = self.style.fill_color {
                out = Some(Pixel(self.p, fill));
            }

            self.p.x += 1;

            // Reached end of row? Jump down one line
            if self.p.x > self.bottom_right.x {
                self.p.x = self.top_left.x;
                self.p.y += 1;
            }

            if out.is_some() {
                break out;
            }
        }
    }
}

pub fn rectangle(fb: &mut LcdFB, rect: Rectangle) {
    let r = RectangleIterator {
        top_left: rect.tl,
        bottom_right: rect.br,
        style: rect.style,
        p: rect.tl,
    };

    for pixel in r {
        put_pixel(fb, pixel.0.x, pixel.0.y, pixel.1);
    }
}
