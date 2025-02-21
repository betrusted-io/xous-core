/*
 * The basic idea is to define a Bitmap as a variable mosaic of Sized Tiles.
 *
 * Each Tile contains an Array of u32 Words, a bounding Rectangle, and a Word width.
 * This is arranged to come in just under 4096 bytes, allowing for the rkyv overhead.
 * Each line of bits across the Tile is packed into an Integer number of u32 Words.
 *
 * The Bitmap contains a bounding Rectangle and a Vec of Tiles. The current implmentation
 * has a very simple tiling strategy - a single vertical strip of full-width tiles.
 * All tiles are the same width and same maximum height - except the last Tile which may
 * have some unused Words at the end of the Array. More space efficient tiling strategies
 * are possible - but likely with a processing and code complexity overhead.
 *
 * author: nworbnhoj
 */

use std::cmp::{max, min};
use std::convert::TryInto;
use std::io::Read;
use std::ops::Deref;

use ux_api::minigfx::*;

mod img;
pub use img::*;
mod decode_png;
pub use decode_png::*;
mod greyscale;
pub use greyscale::*;
mod shrink;
pub use shrink::*;
mod dither;
pub use dither::*;

#[derive(Debug)]
pub struct Bitmap {
    width: usize,
    pub bound: Rectangle,
    tile_bits: usize,
    mosaic: Vec<Tile>,
}

impl Bitmap {
    pub fn new(size: Point) -> Self {
        let mut mosaic: Vec<Tile> = Vec::new();
        let mut tl = Point::new(0, 0);
        let mut br = Point::new(size.x, 0);
        let mut tile_bits = 0;
        while tl.y <= size.y {
            let mut tile = Tile::new(Rectangle::new(tl, br));
            let max_bound = tile.max_bound();
            br = if max_bound.br.y > size.y { size } else { Point::new(size.x, max_bound.br.y) };
            tile.set_bound(Rectangle::new(tl, br));
            if tile_bits == 0 {
                tile_bits = ((br.x - tl.x + 1) * (br.y - tl.y + 1)) as usize;
            }
            mosaic.push(tile);
            tl = Point::new(0, br.y + 1);
            br = Point::new(size.x, tl.y);
        }
        Self { width: size.x as usize + 1, bound: Rectangle::new(Point::new(0, 0), size), tile_bits, mosaic }
    }

    pub fn from_img(img: &Img, fit: Option<Point>) -> Self {
        Bitmap::from_iter(
            img.iter().cloned(),
            img.px_type,
            Point::new(img.width().try_into().unwrap(), img.height().try_into().unwrap()),
            fit,
        )
    }

    pub fn from_png<R: Read>(png: &mut DecodePng<R>, fit: Option<Point>) -> Self {
        // Png Colortypes: 0=Grey, 2=Rgb, 3=Palette, 4=GreyAlpha, 6=Rgba.
        let px_type = match (png.color_type(), png.bit_depth()) {
            (0, 1 | 2 | 4) => PixelType::U0, // Unsupported
            (0, 8) => PixelType::U8,
            (0, 16) => PixelType::U16,
            (2, 8) => PixelType::U8x3,
            (2, 16) => PixelType::U16x3,
            (3, 1 | 2 | 4 | 8) => PixelType::U0, // Unsupported
            (4, 8) => PixelType::U8x2,
            (4, 16) => PixelType::U16x2,
            (6, 8) => PixelType::U8x4,
            (6, 16) => PixelType::U16x4,
            (_, _) => PixelType::U0, // Invalid combination
        };
        let px_size = Point::new(png.width().try_into().unwrap(), png.height().try_into().unwrap());
        Bitmap::from_iter(png, px_type, px_size, fit)
    }

    pub fn from_iter<I: Iterator<Item = u8>>(
        bytes: I,
        px_type: PixelType,
        px_size: Point,
        fit: Option<Point>,
    ) -> Self {
        let burkes = BURKES.to_vec();
        let from_width: usize = px_size.x.try_into().unwrap();
        let (rotate, to_width) = match fit {
            Some(fit) => Self::fit(px_size, fit),
            None => (false, from_width),
        };
        let words = bytes.to_grey(px_type).shrink(from_width, to_width).dither(&burkes, to_width);

        let mut mosaic: Vec<Tile> = Vec::new();

        let to_width: isize = to_width.try_into().unwrap();
        let single_line = Point::new(to_width - 1, 0);
        let mut bound = Rectangle::new(Point::new(0, 0), single_line);
        let mut tile = Tile::new(bound);
        let mut blank_tile = true;
        let (mut x, mut y) = (0, 0);
        for word in words {
            tile.set_word(Point::new(x, y), word);
            blank_tile = false;
            x += BITS_PER_WORD as isize;
            if x >= to_width {
                (x, y) = (0, y + 1);
            }
            if y > tile.max_bound().br.y {
                mosaic.push(tile);
                if y > px_size.y {
                    break;
                }
                bound = Rectangle::new(Point::new(x, y), Point::new(to_width - 1, y));
                tile = Tile::new(bound);
                blank_tile = true;
            }
        }
        if !blank_tile {
            bound.br = Point::new(to_width - 1, y - 1);
            tile.crop(bound);
            mosaic.push(tile);
        }

        bound.tl = Point::new(0, 0);
        let max = tile.max_bound();
        let tile_bits = to_width * (max.br.y - max.tl.y + 1);

        let mut bm = Self {
            width: to_width.try_into().unwrap(),
            bound,
            tile_bits: tile_bits.try_into().unwrap(),
            mosaic,
        };

        if rotate { bm.rotate90() } else { bm }
    }

    fn fit(from: Point, into: Point) -> (bool, usize) {
        let (from_x, from_y) = (from.x as f32, from.y as f32);
        let (into_x, into_y) = (into.x as f32, into.y as f32);
        let portrait_scale = (into_x / from_x).min(into_y / from_y);
        let landscape_scale = (into_x / from_y).min(into_y / from_x);
        if portrait_scale >= 1.0 {
            log::info!("show image as is");
            (false, from.x.try_into().unwrap())
        } else if landscape_scale >= 1.0 {
            log::info!("rotate image");
            (true, from.x.try_into().unwrap())
        } else if portrait_scale >= landscape_scale {
            log::info!("scale image {}", portrait_scale);
            (false, (portrait_scale * from_x) as usize)
        } else {
            log::info!("scale image {} and rotate", landscape_scale);
            (true, (landscape_scale * from_x) as usize)
        }
    }

    #[allow(dead_code)]
    fn area(&self) -> u32 {
        let (x, y) = self.size();
        (x * y) as u32
    }

    pub fn size(&self) -> (usize, usize) { (self.bound.br.x as usize, self.bound.br.y as usize) }

    fn get_tile_index(&self, point: Point) -> usize {
        if self.bound.intersects_point(point) {
            let x = point.x as usize;
            let y = point.y as usize;
            (x + y * self.width) / self.tile_bits
        } else {
            log::warn!("Out of bounds {:?}", point);
            0
        }
    }

    fn hull(mosaic: &Vec<Tile>) -> Rectangle {
        let mut hull_tl = Point::new(isize::MAX, isize::MAX);
        let mut hull_br = Point::new(isize::MIN, isize::MIN);
        let mut tile_area = 0;
        for (_i, tile) in mosaic.iter().enumerate() {
            let tile_bound = tile.bound();
            hull_tl.x = min(hull_tl.x, tile_bound.tl.x);
            hull_tl.y = min(hull_tl.y, tile_bound.tl.y);
            hull_br.x = max(hull_br.x, tile_bound.br.x);
            hull_br.y = max(hull_br.y, tile_bound.br.y);
            tile_area += (1 + tile_bound.br.x - tile_bound.tl.x) * (1 + tile_bound.br.y - tile_bound.tl.y);
        }
        let hull_area = (1 + hull_br.x - hull_tl.x) * (1 + hull_br.y - hull_tl.y);
        if tile_area < hull_area {
            log::warn!("Bitmap Tile gaps: tile_area={} hull_area={} {:?}", tile_area, hull_area, mosaic);
        } else if tile_area > hull_area {
            log::warn!("Bitmap Tile overlap: tile_area={} hull_area={} {:?}", tile_area, hull_area, mosaic);
        }
        Rectangle::new(hull_tl, hull_br)
    }

    pub fn get_tile(&self, point: Point) -> Tile {
        let tile = self.get_tile_index(point);
        self.mosaic.as_slice()[tile]
    }

    fn get_mut_tile(&mut self, point: Point) -> &mut Tile {
        let tile = self.get_tile_index(point);
        &mut self.mosaic.as_mut_slice()[tile]
    }

    pub fn get_line(&self, point: Point) -> Vec<Word> { self.get_tile(point).get_line(point) }

    fn get_word(&self, point: Point) -> Word { self.get_tile(point).get_word(point) }

    fn set_word(&mut self, point: Point, word: Word) { self.get_mut_tile(point).set_word(point, word); }

    pub fn get_pixel(&self, point: Point) -> PixelColor { self.get_tile(point).get_pixel(point) }

    pub fn set_pixel(&mut self, point: Point, color: PixelColor) {
        self.get_mut_tile(point).set_pixel(point, color)
    }

    pub fn translate(&mut self, offset: Point) {
        for tile in self.mosaic.as_mut_slice() {
            tile.translate(offset);
        }
        self.bound.tl.x += offset.x;
        self.bound.tl.y += offset.y;
        self.bound.br.x += offset.x;
        self.bound.br.y += offset.y;
    }

    pub fn rotate90(&mut self) -> Self {
        let bits_per_word: isize = BITS_PER_WORD.try_into().unwrap();
        let (size_x, size_y) = self.size();
        let size_x: isize = size_x.try_into().unwrap();
        let size_y: isize = size_y.try_into().unwrap();
        let mut r90 = Bitmap::new(Point::new(size_y, size_x));
        let (_, r90_size_y) = r90.size();

        let mut x: isize = 0;
        let mut r90_y = 0;
        let mut block: [Word; BITS_PER_WORD] = [0; BITS_PER_WORD];
        while x < size_x {
            let mut y = size_y - 1;
            let mut r90_x = 0;
            // extract a square block of bits - ie 32 x u32 words
            // beginning from bottom-left, and progressing up in strips, from left to right
            while y >= 0 {
                let mut b = 0;
                while b < block.len() {
                    block[b] = if y >= 0 { self.get_word(Point::new(x, y)) } else { 0 };
                    y -= 1;
                    b += 1;
                }

                // rotate the block and write to r90
                // beginning from top-left, and progressing right in strips, from top to bottom
                for w in 0..bits_per_word {
                    if r90_y + w >= r90_size_y.try_into().unwrap() {
                        continue;
                    }
                    let mut word: Word = 0;
                    for b in 0..block.len() {
                        word = word | (((block[b] >> w) & 1) << b);
                    }
                    r90.set_word(Point::new(r90_x, r90_y + w), word);
                }
                r90_x = r90_x + bits_per_word;
            }
            x = x + bits_per_word;
            r90_y = r90_y + bits_per_word;
        }
        r90
    }
}

impl Deref for Bitmap {
    type Target = Vec<Tile>;

    fn deref(&self) -> &Self::Target { &self.mosaic }
}

impl From<[Option<Tile>; 6]> for Bitmap {
    fn from(tiles: [Option<Tile>; 6]) -> Self {
        let mut mosaic: Vec<Tile> = Vec::new();
        let mut tile_size = Point::new(0, 0);
        for t in 0..tiles.len() {
            if tiles[t].is_some() {
                let tile = tiles[t].unwrap();
                mosaic.push(tile);
                if tile_size.x == 0 {
                    tile_size = tile.size();
                }
            }
        }

        Self {
            width: (tile_size.x + 1) as usize,
            bound: Self::hull(&mosaic),
            tile_bits: (tile_size.x + 1) as usize * (tile_size.y + 1) as usize,
            mosaic,
        }
    }
}

impl<'a> From<&Img> for Bitmap {
    fn from(image: &Img) -> Self { Bitmap::from_img(image, None) }
}

// **********************************************************************

#[cfg(test)]
mod tests {
    use super::*;
    #[test]

    fn bitmap_test() {
        let x_size = 100;
        let y_size = 10;
        let bm = Bitmap::new(Point::new(x_size, y_size));
        assert_equal!(bm.size.x, x_size);
        assert_equal!(bm.size.y, y_size);
        assert_equal!(bm.get(5, 5), PixelColor::Light);
        bm.set(5, 5, PixelColor::Dark);
        assert_equal!(bm.get(5, 5), PixelColor::Dark);
    }
}
