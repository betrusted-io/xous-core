// PNG Pong
//
// Copyright © 2019-2021 Jeron Aldaron Lau
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0>, or the Zlib License, <LICENSE-ZLIB
// or http://opensource.org/licenses/Zlib>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

// small modifications made in Error handling

use std::io::{Error, ErrorKind, Result};

/// Standard PNG color types.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ColorType {
    /// greyscale: 1, 2, 4, 8, 16 bit
    Grey = 0u8,
    /// RGB: 8, 16 bit
    Rgb = 2,
    /// palette: 1, 2, 4, 8 bit
    Palette = 3,
    /// greyscale with alpha: 8, 16 bit
    GreyAlpha = 4,
    /// RGB with alpha: 8, 16 bit
    Rgba = 6,
}

impl ColorType {
    /// channels * bytes per channel = bytes per pixel
    pub(crate) fn channels(self) -> u8 {
        match self {
            ColorType::Grey | ColorType::Palette => 1,
            ColorType::GreyAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }

    /// get the total amount of bits per pixel, based on colortype and bitdepth
    /// in the struct
    pub(crate) fn bpp(self, bit_depth: u8) -> u8 {
        assert!((1..=16).contains(&bit_depth));
        /* bits per pixel is amount of channels * bits per channel */
        let ch = self.channels();
        ch * if ch > 1 { if bit_depth == 8 { 8 } else { 16 } } else { bit_depth }
    }

    /// Error if invalid color type / bit depth combination for PNG.
    pub(crate) fn check_png_color_validity(self, bd: u8) -> Result<()> {
        match self {
            ColorType::Grey => {
                if !(bd == 1 || bd == 2 || bd == 4 || bd == 8 || bd == 16) {
                    return Err(Error::new(ErrorKind::InvalidData, "invalid color mode"));
                }
            }
            ColorType::Palette => {
                if !(bd == 1 || bd == 2 || bd == 4 || bd == 8) {
                    return Err(Error::new(ErrorKind::InvalidData, "invalid color mode"));
                }
            }
            ColorType::Rgb | ColorType::GreyAlpha | ColorType::Rgba => {
                if !(bd == 8 || bd == 16) {
                    return Err(Error::new(ErrorKind::InvalidData, "invalid color mode"));
                }
            }
        }
        Ok(())
    }
}
