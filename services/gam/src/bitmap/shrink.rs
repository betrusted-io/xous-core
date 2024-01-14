/*
 * Shrink is an adaptor Iterator to reduce the width & height of a raster image
 *
 * author: nworbnhoj
 */

use std::convert::TryInto;

pub struct Shrink<I> {
    /// iterator over inbound pixels
    iter: I,
    /// width of the outbound image
    out_width: usize,
    /// the scale factor between inbound and outbound images (ie in_width/out_width)
    scale: f32,
    /// a pre-tabulated list of the trailing edge of each inbound strip of pixels
    in_x_cap: Vec<u16>,
    /// the current y coord of the inbound image
    in_y: usize,
    /// the current x coord of the outbound image
    out_x: usize,
    /// the current y coord of the outbound image    
    out_y: usize,
    /// a buffer the width of the outbound image to stove horizontal averages
    buf: Vec<u16>,
    /// the width of the current stri in the inbound image
    y_div: u16,
    /// the x coord of the final pixel in the inbound image
    out_x_last: usize,
}

impl<I: Iterator<Item = u8>> Shrink<I> {
    fn new(iter: I, in_width: usize, out_width: usize) -> Shrink<I> {
        let scale = in_width as f32 / out_width as f32;
        // set up a buffer to average the surrounding pixels
        let buf: Vec<u16> = if scale <= 1.0 { Vec::new() } else { vec![0u16; out_width] };

        // Pretabulate horizontal pixel positions
        let mut in_x_cap: Vec<u16> = Vec::with_capacity(out_width);
        let max_width: u16 = (in_width - 1).try_into().unwrap();
        for out_x in 1..=out_width {
            let in_x: u16 = (scale * out_x as f32) as u16;
            in_x_cap.push((in_x).min(max_width));
        }
        let out_x_last = out_width;
        Self { iter, out_width, scale, in_x_cap, in_y: 0, out_x: 0, out_y: 1, buf, y_div: 0, out_x_last }
    }

    #[allow(dead_code)]
    fn next_xy(&self) -> (usize, usize) { (self.out_x, self.out_y) }
}
/// Adaptor Iterator to shrink an image dimensions from in_width to out_width
impl<I: Iterator<Item = u8>> Iterator for Shrink<I> {
    type Item = u8;

    /// Shrinks an image from in_width to out_width
    /// The algorithm divides the inbound image into vertical and horivontal strips,
    /// correspoding to the columns and rows of the outbound image. Each outbound
    /// pixel is the average of the pixels contained within each intersaction of
    /// vertical and horizontal strips. For example, when in_width = 3 x out_width
    /// each outbound pixel will be the average of 9 pixels in a 3x3 inbound block.
    /// Note that with a non-integer scale the strips will be of variable width ±1.
    fn next(&mut self) -> Option<Self::Item> {
        // if there is no reduction in image size then simple return image as-is
        if self.scale <= 1.0 {
            return match self.iter.next() {
                Some(pixel) => Some(pixel),
                None => None,
            };
        }
        // processed the last inbound pixel
        if self.out_x > self.out_x_last {
            return None;
        }
        // take the average of pixels in the horizontal, and then vertical.
        let in_y_cap = (self.scale * self.out_y as f32) as usize;
        while self.in_y <= in_y_cap {
            let mut in_x = 0;
            for (out_x, in_x_cap) in self.in_x_cap.iter().enumerate() {
                let mut x_total: u16 = 0;
                let mut x_div: u16 = 0;
                while in_x <= *in_x_cap {
                    x_total += match self.iter.next() {
                        Some(pixel) => pixel as u16,
                        None => {
                            self.out_x_last = out_x - 1;
                            0
                        }
                    };
                    in_x += 1;
                    x_div += 1;
                }
                self.buf[out_x] += x_total / x_div;
            }
            self.in_y += 1;
            self.y_div += 1;
        }
        // calculate the average of the sum of pixels in the buffer, and reset buffer
        let pixel: u8 = (self.buf[self.out_x] / self.y_div).try_into().unwrap();
        self.buf[self.out_x] = 0;
        // prepare for the next pixel in the row or column
        self.out_x += 1;
        if self.out_x >= self.out_width {
            self.out_x = 0;
            self.out_y += 1;
            self.y_div = 0;
        }
        Some(pixel)
    }
}

pub trait ShrinkIterator: Iterator<Item = u8> + Sized {
    fn shrink(self, in_width: usize, out_width: usize) -> Shrink<Self> {
        Shrink::new(self, in_width, out_width)
    }
}

impl<I: Iterator<Item = u8>> ShrinkIterator for I {}
