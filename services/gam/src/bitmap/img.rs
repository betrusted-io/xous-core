/*
 * Img is a minimal structure to hold an RGB raster image
 *
 * author: nworbnhoj
 */

use std::ops::Deref;

#[derive(Debug, Clone, Copy)]
pub enum PixelType {
    U0, // Error
    U8,
    U8x2,
    U8x3,
    U8x4,
    U16,
    U16x2,
    U16x3,
    U16x4,
}

/*
 * Image as a minimal flat buffer of u8; accessible by (x, y)
 *
 * author: nworbnhoj
 */

//#[derive(Debug)]
pub struct Img {
    pub pixels: Vec<u8>,
    pub width: usize,
    pub px_type: PixelType,
}

impl Img {
    pub fn new(pixels: Vec<u8>, width: usize, px_type: PixelType) -> Self { Self { pixels, width, px_type } }

    pub fn width(&self) -> usize { self.width }

    pub fn height(&self) -> usize {
        match self.px_type {
            PixelType::U8 => self.pixels.len() / self.width,
            PixelType::U8x2 => self.pixels.len() / (self.width * 2),
            PixelType::U8x3 => self.pixels.len() / (self.width * 3),
            PixelType::U8x4 => self.pixels.len() / (self.width * 4),
            PixelType::U16 => self.pixels.len() / (self.width * 2),
            PixelType::U16x2 => self.pixels.len() / (self.width * 4),
            PixelType::U16x3 => self.pixels.len() / (self.width * 6),
            PixelType::U16x4 => self.pixels.len() / (self.width * 8),
            _ => {
                log::warn!("PixelType not implemented");
                0
            }
        }
    }
}

impl Deref for Img {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target { &self.pixels }
}
