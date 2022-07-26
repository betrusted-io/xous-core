use core::cmp::{max, min};
use graphics_server::api::*;
use graphics_server::PixelColor;
use std::convert::TryInto;
use std::io::Read;
use std::ops::Deref;

mod decode_png;
pub use decode_png::*;

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
            br = if max_bound.br.y > size.y {
                size
            } else {
                Point::new(size.x, max_bound.br.y)
            };
            tile.set_bound(Rectangle::new(tl, br));
            if tile_bits == 0 {
                tile_bits = ((br.x - tl.x + 1) * (br.y - tl.y + 1)) as usize;
            }
            mosaic.push(tile);
            tl = Point::new(0, br.y + 1);
            br = Point::new(size.x, tl.y);
        }
        Self {
            width: size.x as usize + 1,
            bound: Rectangle::new(Point::new(0, 0), size),
            tile_bits,
            mosaic,
        }
    }

    pub fn from_img(img: &Img, fit: Option<Point>) -> Self {
        Bitmap::from_iter(
            img.iter().cloned(),
            img.px_type,
            Point::new(
                img.width().try_into().unwrap(),
                img.height().try_into().unwrap(),
            ),
            fit,
        )
    }

    pub fn from_png<R: Read>(png: &mut DecodePng<R>, fit: Option<Point>) -> Self {
        // Png Colortypes: 0=Grey, 2=Rgb, 3=Palette, 4=GreyAlpha, 6=Rgba.
        let px_type = match (png.color_type(), png.bit_depth()) {
            (0, 1 | 2 | 4 | 8) => PixelType::U8,
            (0, 16) => PixelType::U16,
            (2, 8) => PixelType::U8x3,
            (2, 16) => PixelType::U16x3,
            (3, 1 | 2 | 4 | 8) => PixelType::U8,
            (4, 8) => PixelType::U8x2,
            (4, 16) => PixelType::U16x2,
            (6, 8) => PixelType::U8x4,
            (6, 16) => PixelType::U16x4,
            (_, _) => PixelType::U8, // Error
        };
        let px_size = Point::new(
            png.width().try_into().unwrap(),
            png.height().try_into().unwrap(),
        );
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
        let words = bytes
            .to_grey(px_type)
            .shrink(from_width, to_width)
            .dither(&burkes, to_width);

        let mut mosaic: Vec<Tile> = Vec::new();

        let to_width:i16 = to_width.try_into().unwrap();
        let single_line = Point::new(to_width - 1, 0);
        let mut bound = Rectangle::new(Point::new(0, 0), single_line);
        let mut tile = Tile::new(bound);
        let mut blank_tile = true;
        let (mut x, mut y) = (0, 0);
        for word in words {
            tile.set_word(Point::new(x, y), word);
            blank_tile = false;
            x += BITS_PER_WORD as i16;
            if x >= to_width {
                (x, y) = (0, y + 1);
            }
            if y > tile.max_bound().br.y {
                mosaic.push(tile);
                if y > px_size.y { break; }
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

        if rotate {
            bm.rotate90();
        }
        bm
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

    pub fn size(&self) -> (usize, usize) {
        (self.bound.br.x as usize, self.bound.br.y as usize)
    }

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
            log::warn!(
                "Bitmap Tile gaps: tile_area={} hull_area={} {:?}",
                tile_area,
                hull_area,
                mosaic
            );
        } else if tile_area > hull_area {
            log::warn!(
                "Bitmap Tile overlap: tile_area={} hull_area={} {:?}",
                tile_area,
                hull_area,
                mosaic
            );
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
            width: (tile_size.x + 1) as usize,
            bound: Self::hull(&mosaic),
            tile_bits: (tile_size.x + 1) as usize * (tile_size.y + 1) as usize,
            mosaic: mosaic,
        }
    }
}

impl<'a> From<&Img> for Bitmap {
    fn from(image: &Img) -> Self {
        Bitmap::from_img(image, None)
    }
}

// **********************************************************************

#[derive(Debug, Clone, Copy)]
pub enum PixelType {
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
    pub fn new(pixels: Vec<u8>, width: usize, px_type: PixelType) -> Self {
        Self {
            pixels,
            width,
            px_type,
        }
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        match self.px_type {
            PixelType::U8 => self.pixels.len() / self.width,
            PixelType::U8x3 => self.pixels.len() / (self.width * 3),
            _ => {
                log::warn!("PixelType not implemented");
                0
            }
        }
    }
}

impl Deref for Img {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

// **********************************************************************

pub struct GreyScale<I> {
    iter: I,
    px_type: PixelType,
}

impl<I: Iterator<Item = u8>> GreyScale<I> {
    fn new(iter: I, px_type: PixelType) -> GreyScale<I> {
        Self { iter, px_type }
    }
}

impl<I: Iterator<Item = u8>> Iterator for GreyScale<I> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        // chromatic coversion from RGB to Greyscale
        const R: u32 = 2126;
        const G: u32 = 7152;
        const B: u32 = 722;
        const BLACK: u32 = R + G + B;
        match self.px_type {
            PixelType::U8 => match self.iter.next() {
                Some(gr) => Some(gr),
                None => None,
            },
            PixelType::U8x3 => {
                let r = self.iter.next();
                let g = self.iter.next();
                let b = self.iter.next();
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
            _ => {
                log::warn!("unsupported PixelType {:?}", self.px_type);
                None
            }
        }
    }
}

pub trait GreyScaleIterator: Iterator<Item = u8> + Sized {
    /// converts pixels of PixelType to u8 greyscale
    fn to_grey(self, px_type: PixelType) -> GreyScale<Self> {
        GreyScale::new(self, px_type)
    }
}

impl<I: Iterator<Item = u8>> GreyScaleIterator for I {}

// **********************************************************************

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
        let buf: Vec<u16> = if scale <= 1.0 {
            Vec::new()
        } else {
            vec![0u16; out_width]
        };

        // Pretabulate horizontal pixel positions
        let mut in_x_cap: Vec<u16> = Vec::with_capacity(out_width);
        let max_width: u16 = (in_width - 1).try_into().unwrap();
        for out_x in 1..=out_width {
            let in_x: u16 = (scale * out_x as f32) as u16;
            in_x_cap.push((in_x).min(max_width));
        }
        let out_x_last = out_width;
        Self {
            iter,
            out_width,
            scale,
            in_x_cap,
            in_y: 0,
            out_x: 0,
            out_y: 1,
            buf,
            y_div: 0,
            out_x_last,
        }
    }

    #[allow(dead_code)]
    fn next_xy(&self) -> (usize, usize) {
        (self.out_x, self.out_y)
    }
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

// **********************************************************************

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

        Self {
            iter,
            width,
            diffusion,
            denominator,
            err: vec![0i16; length],
            origin: 0,
            next_x: 0,
            next_y: 0,
        }
    }

    #[allow(dead_code)]
    fn next_xy(&self) -> (usize, usize) {
        (self.next_x, self.next_y)
    }

    fn index(&self, dx: isize, dy: isize) -> usize {
        let width: isize = self.width.try_into().unwrap();
        let offset: usize = (width * dy + dx).try_into().unwrap();
        let linear: usize = self.origin + offset;
        linear % self.err.len()
    }
    fn err(&self) -> i16 {
        self.err[self.origin] / self.denominator
    }
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
