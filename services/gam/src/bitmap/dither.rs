/*
 * Dithering involves aplying a threshold to each pixel to round down to Black
 * or up to White. The residual error from this blunt instrument is diffused amongst
 * the surrounding pixels. So the luminosity lost by forcing a pixel down to Black,
 * results in the surrounding pixels incrementally more likely to round up to White.
 * Pixels are processed from left to right and then top to bottom. The residual
 * error from each Black/White determination is carried-forward to pixels to the
 * right and below as per the diffusion scheme.
 * https://tannerhelland.com/2012/12/28/dithering-eleven-algorithms-source-code.html
 *
 * author: nworbnhoj
 */

use std::cmp::max;
use std::convert::TryInto;

use crate::bitmap::BITS_PER_WORD;
use crate::PixelColor;

/// Burkes dithering diffusion scheme was chosen for its modest resource
/// requirements with impressive quality outcome.
/// Burkes dithering. Div=32.
/// - ` .  .  x  8  4`
/// - ` 2  4  8  4  2`
pub const BURKES: [(isize, isize, i16); 7] = [
    // (dx, dy, mul)
    (1, 0, 8),
    (2, 0, 4),
    //
    (-2, 1, 2),
    (-1, 1, 4),
    (0, 1, 8),
    (1, 1, 4),
    (2, 1, 2),
];

pub struct Dither<'a, I> {
    /// iterator over inbound pixels
    iter: I,
    // the width of the image to be dithered
    width: usize,
    // the error diffusion scheme (dx, dy, multiplier)
    diffusion: &'a Vec<(isize, isize, i16)>,
    // the sum of the multipliers in the diffusion
    denominator: i16,
    // a circular array of errors representing dy rows of the image,
    err: Vec<i16>,
    // the position in err representing the carry forward error for the current pixel
    origin: usize,
    next_x: usize,
    next_y: usize,
}

const THRESHOLD: i16 = u8::MAX as i16 / 2;

impl<'a, I: Iterator<Item = u8>> Dither<'a, I> {
    //    const THRESHOLD: i16 = u8::MAX as i16 / 2; results in:  cannot satisfy `<_ as Iterator>::Item == u8`
    fn new(iter: I, diffusion: &'a Vec<(isize, isize, i16)>, width: usize) -> Dither<I> {
        let mut denominator: i16 = 0;
        for (_, _, mul) in diffusion {
            denominator += mul;
        }
        let (mut max_dx, mut max_dy) = (0, 0);
        for (dx, dy, _) in diffusion {
            max_dx = max(*dx, max_dx);
            max_dy = max(*dy, max_dy);
        }
        let length: usize = width * max_dy as usize + max_dx as usize + 1;

        Self { iter, width, diffusion, denominator, err: vec![0i16; length], origin: 0, next_x: 0, next_y: 0 }
    }

    #[allow(dead_code)]
    fn next_xy(&self) -> (usize, usize) { (self.next_x, self.next_y) }

    fn index(&self, dx: isize, dy: isize) -> usize {
        let width: isize = self.width.try_into().unwrap();
        let offset: usize = (width * dy + dx).try_into().unwrap();
        let linear: usize = self.origin + offset;
        linear % self.err.len()
    }

    fn err(&self) -> i16 { self.err[self.origin] / self.denominator }

    fn carry(&mut self, err: i16) {
        for (dx, dy, mul) in self.diffusion {
            let i = self.index(*dx, *dy);
            self.err[i] += mul * err;
        }
    }

    fn pixel(&mut self, grey: u8) -> PixelColor {
        let grey: i16 = grey as i16 + self.err();
        if grey < THRESHOLD {
            self.carry(grey);
            PixelColor::Dark
        } else {
            self.carry(grey - u8::MAX as i16);
            PixelColor::Light
        }
    }
}

impl<'a, I: Iterator<Item = u8>> Iterator for Dither<'a, I> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        let mut word = 0;
        for w in 0..BITS_PER_WORD {
            match self.iter.next() {
                Some(grey) => {
                    let color = self.pixel(grey) as u32;
                    word = word | (color << w);
                }
                None => {
                    if w > 0 {
                        continue;
                    } else {
                        return None;
                    }
                }
            };

            // reset and step forward err buffer and next_x coord
            self.err[self.origin] = 0;
            self.origin = self.index(1, 0);
            self.next_x += 1;
            if self.next_x >= self.width {
                break;
            }
        }
        if self.next_x >= self.width {
            self.next_x = 0;
            self.next_y += 1;
        }
        Some(word)
    }
}

pub trait DitherIterator<'a>: Iterator<Item = u8> + Sized {
    fn dither(self, diffusion: &'a Vec<(isize, isize, i16)>, width: usize) -> Dither<'a, Self> {
        Dither::new(self, diffusion, width)
    }
}

impl<'a, I: Iterator<Item = u8>> DitherIterator<'a> for I {}
