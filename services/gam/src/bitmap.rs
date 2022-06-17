use core::cmp::{max, min};
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

impl From<&Img> for Bitmap {
    fn from(image: &Img) -> Self {
        let (bm_width, bm_height, _) = image.size();
        let bm_bottom = bm_height - 1;
        let bm_right = bm_width - 1;
        let bm_br = Point::new(bm_right as i16, bm_bottom as i16);
        let bound = Rectangle::new(Point::new(0, 0), bm_br);

        let (tile_size, tile_width_words) = Bitmap::tile_spec(bm_br);
        let tile_height: usize = tile_size.y.try_into().unwrap();
        let tile_count = match bm_height % tile_height {
            0 => bm_height / tile_height,
            _ => bm_height / tile_height + 1,
        };
        let mut mosaic: Vec<Tile> = Vec::new();

        let pixels = Dither::new(BURKES.to_vec()).dither(&image);
        let mut px_index = 0;
        let bits_per_word: i16 = BITS_PER_WORD.try_into().unwrap();
        for t in 0..tile_count {
            let t_top = t * tile_height;
            let t_left = 0;
            let t_bottom = min(bm_bottom, (t + 1) * tile_height - 1);
            let t_right = tile_size.x - 1;
            let t_tl = Point::new(t_left, t_top.try_into().unwrap());
            let t_br = Point::new(t_right, t_bottom.try_into().unwrap());
            let t_bound = Rectangle::new(t_tl, t_br);
            let mut tile = Tile::new(t_bound, tile_width_words.try_into().unwrap());
            for y in t_top..=t_bottom {
                let mut x = t_left;
                while x <= t_right {
                    let mut word: usize = 0;
                    for w in 0..bits_per_word {
                        if (x + w) > t_right {
                            continue;
                        }
                        let color = pixels[px_index] as usize;
                        word = word | (color << w);
                        px_index += 1;
                    }
                    let anchor = Point::new(x.try_into().unwrap(), y.try_into().unwrap());
                    tile.set_word(anchor, word.try_into().unwrap());
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

pub enum PixelType {
    U8,
    U8x3,
    U8x4,
}

/*
 * Image as a minimal flat buffer of grey u8 pixels; accessible by (x, y)
 *
 * author: nworbnhoj
 */

#[derive(Debug, Clone)]
pub struct Img {
    pixels: Vec<u8>,
    width: usize,
}

impl Img {
    pub fn new(buf: Vec<u8>, width: usize, px_type: PixelType) -> Self {
        const R: u32 = 2126;
        const G: u32 = 7152;
        const B: u32 = 722;
        const BLACK: u32 = R + G + B;
        let px_len = match px_type {
            PixelType::U8 => buf.len(),
            PixelType::U8x3 => buf.len() / 3,
            _ => 0,
        };
        let mut pixels: Vec<u8> = Vec::with_capacity(px_len);
        for px in 0..px_len {
            let pixel: u8 = match px_type {
                PixelType::U8 => buf[px],
                PixelType::U8x3 => {
                    let b = px * 3;
                    let grey_r = R * buf[b] as u32;
                    let grey_g = G * buf[b + 1] as u32;
                    let grey_b = B * buf[b + 2] as u32;
                    ((grey_r + grey_g + grey_b) / BLACK).try_into().unwrap()
                }
                _ => {
                    log::warn!("unsupported PixelType");
                    0
                }
            };
            pixels.push(pixel);
        }
        Self { pixels, width }
    }
    pub fn get(&self, x: usize, y: usize) -> Option<&u8> {
        let i: usize = (y * self.width) + x;
        self.pixels.get(i)
    }
    pub fn size(&self) -> (usize, usize, usize) {
        let width: usize = self.width.try_into().unwrap();
        let length: usize = self.pixels.len().try_into().unwrap();
        let height: usize = length / width;
        (width, height, length)
    }
    pub fn as_slice(&self) -> &[u8] {
        self.pixels.as_slice()
    }
}

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

/// Burkes dithering diffusion scheme was chosen for its modest resource
/// requirements with impressive quality outcome.
/// Burkes dithering. Div=32.
/// - ` .  .  x  8  4`
/// - ` 2  4  8  4  2`
const BURKES: [(isize, isize, i16); 7] = [
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

struct Dither {
    // the width of the image to be dithered
    width: usize,
    // the error diffusion scheme (dx, dy, multiplier)
    diffusion: Vec<(isize, isize, i16)>,
    // the sum of the multipliers in the diffusion
    denominator: i16,
    // a circular array of errors representing dy rows of the image,
    err: Vec<i16>,
    // the position in err representing the carry forward error for the current pixel
    origin: usize,
}

impl Dither {
    const THRESHOLD: i16 = u8::MAX as i16 / 2;
    pub fn new(diffusion: Vec<(isize, isize, i16)>) -> Self {
        let mut denominator: i16 = 0;
        for (_, _, mul) in &diffusion {
            denominator += mul;
        }
        Self {
            width: 0,
            diffusion,
            denominator,
            err: Vec::<i16>::new(),
            origin: 0,
        }
    }
    fn provision(&mut self, width: usize) {
        self.width = width;
        let (mut max_dx, mut max_dy) = (0, 0);
        for (dx, dy, _) in &self.diffusion {
            max_dx = max(*dx, max_dx);
            max_dy = max(*dy, max_dy);
        }
        let length: usize = width * max_dy as usize + max_dx as usize + 1;
        self.err = vec![0i16; length];
    }
    fn index(&self, dx: isize, dy: isize) -> usize {
        let width: isize = self.width.try_into().unwrap();
        let offset: usize = (width * dy + dx).try_into().unwrap();
        let linear: usize = self.origin + offset;
        linear % self.err.len()
    }
    fn next(&mut self) {
        self.err[self.origin] = 0;
        self.origin = self.index(1, 0);
    }
    fn get(&self) -> i16 {
        self.err[self.origin] / self.denominator
    }
    fn carry(&mut self, err: i16) {
        for (dx, dy, mul) in &self.diffusion {
            let i = self.index(*dx, *dy);
            self.err[i] += mul * err;
        }
    }
    fn pixel(&mut self, grey: u8) -> PixelColor {
        let grey: i16 = grey as i16 + self.get();
        if grey < Dither::THRESHOLD {
            self.carry(grey);
            PixelColor::Dark
        } else {
            self.carry(grey - u8::MAX as i16);
            PixelColor::Light
        }
    }
    pub fn dither(&mut self, image: &Img) -> Vec<PixelColor> {
        let (width, height, length) = image.size();
        self.provision(width.try_into().unwrap());
        let mut pixels: Vec<PixelColor> = Vec::with_capacity(length);
        for y in 0..height {
            for x in 0..width {
                let pixel = match image.get(x, y) {
                    Some(grey) => self.pixel(*grey),
                    None => PixelColor::Dark,
                };
                pixels.push(pixel);
                self.next();
            }
        }
        pixels
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
