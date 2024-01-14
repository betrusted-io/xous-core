/*
 * GreyScale is an adaptor Iterator to convert RGB bytes to a greyscale byte
 *
 * author: nworbnhoj
 */

use std::convert::TryInto;

use crate::PixelType;

pub struct GreyScale<I> {
    iter: I,
    px_type: PixelType,
}

impl<I: Iterator<Item = u8>> GreyScale<I> {
    fn new(iter: I, px_type: PixelType) -> GreyScale<I> { Self { iter, px_type } }
}

impl<I: Iterator<Item = u8>> Iterator for GreyScale<I> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        match self.px_type {
            PixelType::U8 => match self.iter.next() {
                Some(gr) => Some(gr),
                None => None,
            },
            PixelType::U8x2 => match self.iter.next() {
                Some(gr) => {
                    let _alpha = self.iter.next();
                    Some(gr)
                }
                None => None,
            },
            PixelType::U8x3 => {
                let r = self.iter.next();
                let g = self.iter.next();
                let b = self.iter.next();
                grey(r, g, b)
            }
            PixelType::U8x4 => {
                let r = self.iter.next();
                let g = self.iter.next();
                let b = self.iter.next();
                let _alpha = self.iter.next();
                grey(r, g, b)
            }
            PixelType::U16 => match self.iter.next() {
                Some(gr) => {
                    let _lower_bits = self.iter.next();
                    Some(gr)
                }
                None => None,
            },
            PixelType::U16x2 => match self.iter.next() {
                Some(gr) => {
                    let _lower_bits = self.iter.next();
                    let _alpha = self.iter.next();
                    let _alpha = self.iter.next();
                    Some(gr)
                }
                None => None,
            },
            PixelType::U16x3 => {
                let r = self.iter.next();
                let _lower_bits = self.iter.next();
                let g = self.iter.next();
                let _lower_bits = self.iter.next();
                let b = self.iter.next();
                let _lower_bits = self.iter.next();
                grey(r, g, b)
            }
            PixelType::U16x4 => {
                let r = self.iter.next();
                let _lower_bits = self.iter.next();
                let g = self.iter.next();
                let _lower_bits = self.iter.next();
                let b = self.iter.next();
                let _lower_bits = self.iter.next();
                let _alpha = self.iter.next();
                let _lower_bits = self.iter.next();
                grey(r, g, b)
            }
            _ => {
                log::warn!("unsupported PixelType {:?}", self.px_type);
                None
            }
        }
    }
}

// chromatic coversion from RGB to Greyscale
fn grey(r: Option<u8>, g: Option<u8>, b: Option<u8>) -> Option<u8> {
    const R: u32 = 2126;
    const G: u32 = 7152;
    const B: u32 = 722;
    const BLACK: u32 = R + G + B;
    if r.is_some() && g.is_some() && b.is_some() {
        let grey_r = R * r.unwrap() as u32;
        let grey_g = G * g.unwrap() as u32;
        let grey_b = B * b.unwrap() as u32;
        let grey: u8 = ((grey_r + grey_g + grey_b) / BLACK).try_into().unwrap();
        Some(grey)
    } else {
        None
    }
}

pub trait GreyScaleIterator: Iterator<Item = u8> + Sized {
    /// converts pixels of PixelType to u8 greyscale
    fn to_grey(self, px_type: PixelType) -> GreyScale<Self> { GreyScale::new(self, px_type) }
}

impl<I: Iterator<Item = u8>> GreyScaleIterator for I {}
