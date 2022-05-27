use crate::api::*;
use std::convert::TryInto;
//////////////////////// Tile -------- author: nworbnhoj

/*
 * A Tile contains an Array of u32 Words, a bounding Rectangle, and a Word width.
 * This is arranged to come in just under 4096 bytes, allowing for the rkyv overhead.
 * Each line of bits across the Tile is packed into an Integer number of u32 Words.
 * Hence, there may be unused bits in the right-most Word in each line, and a few
 * unused Words at the end of the Array. This arrangement is similar to the structure
 * of the frame-buffer in the graphics-server to facilitate efficient transfer.
 */

pub type Word = u32;
pub const BITS_PER_WORD: usize = Word::BITS as usize;
pub(crate) const META_WORDS_PER_TILE: usize = 4;
pub const WORDS_PER_TILE: usize = (4096 * 8 / BITS_PER_WORD) - META_WORDS_PER_TILE;
pub const BITS_PER_TILE: usize = WORDS_PER_TILE * BITS_PER_WORD;
pub(crate) const OUT_OF_BOUNDS: usize = usize::MAX;

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Tile {
    bound: Rectangle,
    width_words: u16,
    words: [Word; WORDS_PER_TILE],
}

impl Tile {
    pub fn new(bound: Rectangle, width_words: u16) -> Self {
        log::trace!("new Tile {:?}", bound);
        let words = [0u32; WORDS_PER_TILE];
        Self {
            bound,
            width_words,
            words,
        }
    }

    pub fn bound(&self) -> Rectangle {
        self.bound
    }

    pub fn area(&self) -> u32 {
        let size = self.size();
        (size.x * size.y).try_into().unwrap()
    }

    pub fn size(&self) -> Point {
        let pos = self.bound;
        Point::new(pos.br.x - pos.tl.x, pos.br.y - pos.tl.y)
    }

    fn word_index(&self, point: Point) -> usize {
        if self.bound.intersects_point(point) {
            let tile_line: u16 = (point.y - self.bound.tl.y).try_into().unwrap();
            let first_word_in_line:usize = (tile_line * self.width_words).try_into().unwrap();
            let bit_index: usize = (point.x - self.bound.tl.x).try_into().unwrap();
            first_word_in_line + (bit_index / BITS_PER_WORD)
        } else {
            log::warn!("point {:?} out of bounds {:?}", point, self.bound);
            OUT_OF_BOUNDS
        }
    }

    pub fn get_word(&self, point: Point) -> Word {
        self.words[self.word_index(point)]
    }

    pub fn set_word(&mut self, point: Point, word: Word) {
        self.words[self.word_index(point)] = word;
    }

    pub fn get_line(&self, point: Point) -> Vec<Word> {
        let first_pixel_in_line = Point::new(self.bound.tl.x, point.y);
        let first_word = self.word_index(first_pixel_in_line);
        let width: usize = self.width_words.try_into().unwrap();
        match first_word == OUT_OF_BOUNDS {
            true => Vec::new(),
            false => {
                let last_word = first_word + width;
                self.words[first_word..last_word].to_vec()
            }
        }
    }

    pub fn set_line(&self, _point: Point, _pixels: Vec<PixelColor>){
        log::warn!("not implemented");
    }

    pub fn get_pixel(&self, point: Point) -> PixelColor {
        let word: usize = self.get_word(point).try_into().unwrap();
        let bpw: i16 =  BITS_PER_WORD.try_into().unwrap();
        let bit = point.x % bpw;
        PixelColor::from((word >> bit) & 1)
    }

    pub fn set_pixel(&mut self, point: Point, color: PixelColor) {
        let word_index = self.word_index(point);
        let bpw: i16 =  BITS_PER_WORD.try_into().unwrap();
        match word_index == OUT_OF_BOUNDS {
            true => {}
            false => {
                let word = self.words[word_index];
                let bit = point.x % bpw;
                match color {
                    PixelColor::Dark => self.words[word_index] = word | 1 << bit,
                    PixelColor::Light => self.words[word_index] = word & !(1 << bit),
                }
            }
        };
    }

    pub fn translate(&mut self, offset: Point) {
        self.bound.tl.x += offset.x;
        self.bound.tl.y += offset.y;
        self.bound.br.x += offset.x;
        self.bound.br.y += offset.y;
    }
}
