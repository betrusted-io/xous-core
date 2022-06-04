use core::cmp::{max, min};
use dither::prelude::{Dither, Img, RGB};
use graphics_server::api::*;
use graphics_server::PixelColor;
use std::convert::TryInto;
use std::ops::Deref;

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

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub bound: Rectangle,
    tile_size: Point,
    mosaic: Vec<Tile>,
}

impl Bitmap {
    pub fn new(size: Point) -> Self {
        let bound = Rectangle::new(Point::new(0, 0), size);
        log::trace!("new Bitmap {:?}", bound);

        let (tile_size, tile_width_words) = Bitmap::tile_spec(size);
        let tile_height = tile_size.y as usize;
        let bm_height = (size.y + 1) as usize;
        let tile_count = match bm_height % tile_height {
            0 => bm_height / tile_height,
            _ => bm_height / tile_height + 1,
        };

        let mut mosaic: Vec<Tile> = Vec::new();
        for y in 0..tile_count {
            let tl = Point::new(0, (y * tile_height) as i16);
            let mut br = Point::new(tile_size.x - 1, ((y + 1) * tile_height - 1) as i16);
            if br.y > size.y {
                br.y = size.y;
            }
            let tile = Tile::new(Rectangle::new(tl, br), tile_width_words as u16);
            mosaic.push(tile);
        }
        Self {
            bound,
            tile_size,
            mosaic,
        }
    }

    fn tile_spec(bm_size: Point) -> (Point, i16) {
        let bm_width_bits = 1 + bm_size.x as usize;
        let mut tile_width_bits = bm_width_bits;
        let tile_width_words = if bm_width_bits > BITS_PER_TILE {
            log::warn!("Bitmap max width exceeded");
            tile_width_bits = WORDS_PER_TILE * BITS_PER_WORD;
            WORDS_PER_TILE
        } else {
            match bm_width_bits % BITS_PER_WORD {
                0 => bm_width_bits / BITS_PER_WORD,
                _ => bm_width_bits / BITS_PER_WORD + 1,
            }
        };
        let tile_height_bits = WORDS_PER_TILE / tile_width_words;
        let tile_size = Point::new(tile_width_bits as i16, tile_height_bits as i16);
        (tile_size, tile_width_words as i16)
    }

    #[allow(dead_code)]
    fn area(&self) -> u32 {
        let (x, y) = self.size();
        (x * y) as u32
    }

    pub fn size(&self) -> (usize, usize) {
        (self.bound.br.x as usize, self.bound.br.y as usize)
    }

    fn get_tile_index(&self, point: Point) -> usize {
        if self.bound.intersects_point(point) {
            let x = point.x as usize;
            let y = point.y as usize;
            let tile_width = self.tile_size.x as usize;
            let tile_height = self.tile_size.y as usize;
            let tile_size_bits = tile_width * tile_height;
            (x + y * tile_width) / tile_size_bits
        } else {
            log::warn!("Out of bounds {:?}", point);
            0
        }
    }

    fn hull(mosaic: &Vec<Tile>) -> Rectangle {
        let mut hull_tl = Point::new(i16::MAX, i16::MAX);
        let mut hull_br = Point::new(i16::MIN, i16::MIN);
        let mut tile_area = 0;
        for (_i, tile) in mosaic.iter().enumerate() {
            let tile_bound = tile.bound();
            hull_tl.x = min(hull_tl.x, tile_bound.tl.x);
            hull_tl.y = min(hull_tl.y, tile_bound.tl.y);
            hull_br.x = max(hull_br.x, tile_bound.br.x);
            hull_br.y = max(hull_br.y, tile_bound.br.y);
            tile_area +=
                (1 + tile_bound.br.x - tile_bound.tl.x) * (1 + tile_bound.br.y - tile_bound.tl.y);
        }
        let hull_area = (1 + hull_br.x - hull_tl.x) * (1 + hull_br.y - hull_tl.y);
        if tile_area < hull_area {
            log::warn!("Bitmap Tile gaps");
        } else if tile_area > hull_area {
            log::warn!("Bitmap Tile overlap");
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

    pub fn get_line(&self, point: Point) -> Vec<Word> {
        self.get_tile(point).get_line(point)
    }

    fn get_word(&self, point: Point) -> Word {
        self.get_tile(point).get_word(point)
    }

    fn set_word(&mut self, point: Point, word: Word) {
        self.get_mut_tile(point).set_word(point, word);
    }

    pub fn get_pixel(&self, point: Point) -> PixelColor {
        self.get_tile(point).get_pixel(point)
    }

    pub fn set_pixel(&mut self, point: Point, color: PixelColor) {
        self.get_mut_tile(point).set_pixel(point, color)
    }

    pub fn translate(&mut self, offset: Point) {
        for tile in self.mosaic.as_mut_slice() {
            tile.translate(offset);
        }
    }

    pub fn rotate90(&mut self) -> Self {
        let bits_per_word: i16 = BITS_PER_WORD.try_into().unwrap();
        let (size_x, size_y) = self.size();
        let size_x: i16 = size_x.try_into().unwrap();
        let size_y: i16 = size_y.try_into().unwrap();
        let mut r90 = Bitmap::new(Point::new(size_y, size_x));
        let (_, r90_size_y) = r90.size();

        let mut x: i16 = 0;
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
                    block[b] = if y >= 0 {
                        self.get_word(Point::new(x, y))
                    } else {
                        0
                    };
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

    fn deref(&self) -> &Self::Target {
        &self.mosaic
    }
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
            bound: Self::hull(&mosaic),
            tile_size: tile_size,
            mosaic: mosaic,
        }
    }
}

impl From<Img<RGB<u8>>> for Bitmap {
    fn from(image: Img<RGB<u8>>) -> Self {
        let img = image.convert_with(|rgb: RGB<u8>| rgb.convert_with(f64::from));
        let bit_depth = 1;
        let quantize = dither::create_quantize_n_bits_func(bit_depth).unwrap();
        let bw_img = img.convert_with(|rgb| rgb.to_chroma_corrected_black_and_white());
        let ditherer = dither::ditherer::BURKES;
        let output_img = ditherer.dither(bw_img, quantize);

        let bm_width: usize = output_img.width().try_into().unwrap();
        let img_vec = output_img.into_vec();

        let bm_height = img_vec.len() / bm_width;
        let bm_bottom = (bm_height - 1) as i16;
        let bm_right = (bm_width - 1) as i16;
        let bm_br = Point::new(bm_right, bm_bottom);
        let bound = Rectangle::new(Point::new(0, 0), bm_br);

        let (tile_size, tile_width_words) = Bitmap::tile_spec(bm_br);
        let tile_height = tile_size.y as usize;
        let tile_count = match bm_height % tile_height {
            0 => bm_height / tile_height,
            _ => bm_height / tile_height + 1,
        };
        let mut mosaic: Vec<Tile> = Vec::new();

        let mut img_vec_index = 0;
        let bits_per_word: i16 = BITS_PER_WORD.try_into().unwrap();
        let dark: usize = PixelColor::Dark.try_into().unwrap();
        let light: usize = PixelColor::Light.try_into().unwrap();
        for t in 0..tile_count {
            let t_top = (t * tile_height) as i16;
            let t_left = 0;
            let t_bottom = min(bm_bottom, ((t + 1) * tile_height - 1) as i16);
            let t_right = tile_size.x - 1;
            let t_tl = Point::new(t_left, t_top);
            let t_br = Point::new(t_right, t_bottom);
            let t_bound = Rectangle::new(t_tl, t_br);
            let mut tile = Tile::new(t_bound, tile_width_words as u16);
            for y in t_top..=t_bottom {
                let mut x = t_left;
                while x <= t_right {
                    let mut word: usize = 0;
                    for w in 0..bits_per_word {
                        if (x + w) > t_right {
                            continue;
                        }
                        let pixel = img_vec[img_vec_index];
                        let color = if pixel > 125.0 { dark } else { light };
                        word = word | (color << w);
                        img_vec_index += 1;
                    }
                    tile.set_word(Point::new(x, y), word.try_into().unwrap());
                    x += bits_per_word;
                }
            }
            mosaic.push(tile);
        }
        Self {
            bound,
            tile_size,
            mosaic,
        }
    }
}

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
