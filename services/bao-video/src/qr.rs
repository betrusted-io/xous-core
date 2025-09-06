use core::ops::{BitXor, Not};

use ux_api::minigfx::{FrameBuffer, Point};

use crate::gfx;

const SEQ_LEN: usize = 5;

// use fixed point for the maths. This defines where we fix the point at.
const SEQ_FP_SHIFT: usize = 4;
// searching for a 1:1:3:1:1 black-white-black-white-black pattern
// upper/lower thresholds for recognizing a "1" in the ratio
const LOWER_1: usize = (1 << SEQ_FP_SHIFT) / 2; // "0.5"
const UPPER_1: usize = 2 << SEQ_FP_SHIFT;
// upper/lower thresholds for recognizing a "3" in the ratio
const LOWER_3: usize = 2 << SEQ_FP_SHIFT;
const UPPER_3: usize = 4 << SEQ_FP_SHIFT;

#[allow(dead_code)]
pub const CROSSHAIR_LEN: isize = 3;
#[allow(dead_code)]
pub fn draw_crosshair(image: &mut dyn FrameBuffer, p: Point) {
    use ux_api::minigfx::Line;
    gfx::line(
        image,
        Line::new(p + Point::new(0, CROSSHAIR_LEN), p - Point::new(0, CROSSHAIR_LEN)),
        None,
        true,
    );
    gfx::line(
        image,
        Line::new(p + Point::new(CROSSHAIR_LEN, 0), p - Point::new(CROSSHAIR_LEN, 0)),
        None,
        true,
    );
}

#[derive(Copy, Clone, Default, Debug)]
struct FinderSeq {
    /// run length of the pixels leading up to the current position
    pub run: usize,
    /// the position
    pub pos: usize,
    /// the luminance of the pixels in the run leading up to the current position
    pub color: Color,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Color {
    Black,
    White,
}
impl Color {
    pub fn from(luminance: u8, thresh: u8) -> Self {
        if luminance > thresh { Color::White } else { Color::Black }
    }
}
/// Used for counting pixels
impl Into<usize> for Color {
    fn into(self) -> usize {
        match self {
            Color::Black => 0,
            Color::White => 1,
        }
    }
}
/// Used for translating pixels into RGB or Luma color spaces
impl Into<u8> for Color {
    fn into(self) -> u8 {
        match self {
            Color::Black => 0,
            Color::White => 255,
        }
    }
}
impl Default for Color {
    fn default() -> Self { Color::Black }
}

impl Not for Color {
    type Output = Color;

    fn not(self) -> Self::Output {
        match self {
            Color::Black => Color::White,
            Color::White => Color::Black,
        }
    }
}

impl BitXor for Color {
    type Output = Color;

    fn bitxor(self, rhs: Self) -> Self::Output { if self == rhs { Color::Black } else { Color::White } }
}

struct SeqBuffer {
    buffer: [Option<FinderSeq>; 5],
    start: usize,
    end: usize,
    count: usize,
}

impl SeqBuffer {
    pub const fn new() -> Self { SeqBuffer { buffer: [None; SEQ_LEN], start: 0, end: 0, count: 0 } }

    pub fn clear(&mut self) {
        self.buffer = [None; SEQ_LEN];
        self.start = 0;
        self.end = 0;
        self.count = 0;
    }

    pub fn push(&mut self, item: FinderSeq) {
        self.buffer[self.end] = Some(item);
        self.end = (self.end + 1) % SEQ_LEN;

        if self.count < SEQ_LEN {
            self.count += 1;
        } else {
            self.start = (self.start + 1) % SEQ_LEN; // Overwrite the oldest element
        }
    }

    /// Don't use options because we iterate through the list once to extract the
    /// correct ordering, and we know how many valid items are in there. This is less
    /// idiomatic, but it saves us the computational overhead of constantly iterating
    /// through to test for None when we know how many there are in the first place,
    /// and it saves the lexical verbosity of `unwrap()` everywhere (and unwrap does
    /// actually have a computational cost, it is not a zero-cost abstraction).
    pub fn retrieve(&self, output: &mut [FinderSeq; SEQ_LEN]) -> usize {
        let mut copied_count = 0;

        for i in 0..self.count {
            let index = (self.start + i) % SEQ_LEN;
            output[i] = self.buffer[index].expect("circular buffer logic error").clone();
            copied_count += 1;
        }

        for i in copied_count..5 {
            output[i] = FinderSeq::default(); // Clear the remaining elements in the output if any
        }

        copied_count
    }

    // returns a tuple of (center point of the sequence, total length of sequence)
    pub fn search(&self) -> Option<(usize, usize)> {
        let mut ratios = [0usize; SEQ_LEN];
        let mut seq: [FinderSeq; SEQ_LEN] = [FinderSeq::default(); SEQ_LEN];
        if self.retrieve(&mut seq) == SEQ_LEN {
            // only look for sequences that start with black
            if seq[0].color == Color::Black {
                let denom = seq[0].run;
                ratios[0] = 1 << SEQ_FP_SHIFT; // by definition
                for (ratio, s) in ratios[1..].iter_mut().zip(seq[1..].iter()) {
                    *ratio = (s.run << SEQ_FP_SHIFT) / denom;
                }
                if ratios[1] >= LOWER_1
                    && ratios[1] <= UPPER_1
                    && ratios[2] >= LOWER_3
                    && ratios[2] <= UPPER_3
                    && ratios[3] >= LOWER_1
                    && ratios[3] <= UPPER_1
                    && ratios[4] >= LOWER_1
                    && ratios[4] <= UPPER_1
                {
                    // log::info!("  seq {:?}", &seq);
                    // log::info!("  ratios {:?}", &ratios);
                    Some((seq[2].pos - seq[2].run / 2 - 1, seq.iter().map(|s| s.run).sum()))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// Returns the average width of the finder regions found.
pub fn find_finders(candidates: &mut Vec<Point>, image: &[u8], thresh: u8, stride: usize) -> usize {
    let mut row_candidates = Vec::<Point>::new();

    let mut seq_buffer = SeqBuffer::new();

    // log::info!("ROWSROWSROWSROWS");
    for (y, line) in image.chunks(stride).enumerate() {
        seq_buffer.clear();
        let mut last_color = Color::from(line[0], thresh);
        let mut run_length = 1;

        for (x_minus1, &pix) in line[1..].iter().enumerate() {
            let pix = Color::from(pix, thresh);
            if pix == last_color {
                run_length += 1;
            } else {
                seq_buffer.push(FinderSeq { run: run_length, pos: x_minus1 + 1, color: last_color });
                last_color = pix;
                run_length = 1;

                // manage the sequence index
                if let Some((pos, _width)) = seq_buffer.search() {
                    // log::info!("row candidate {}, {}", pos, y);
                    row_candidates.push(Point::new(pos as _, y as _));
                }
            }
        }
    }

    // log::info!("CCCCCCCCCCCCCCCC");
    let mut candidate_width = 0;
    for x in 0..stride {
        seq_buffer.clear();

        let mut last_color = Color::from(image[x], thresh);
        let mut run_length = 1;
        // could rewrite this to abort the search after more than 3 finders are found, but for now,
        // do an exhaustive search because it's useful info for debugging.
        for (y_minus1, row) in image[x + stride..].chunks(stride).enumerate() {
            let pix = Color::from(row[0], thresh);
            if pix == last_color {
                run_length += 1;
            } else {
                seq_buffer.push(FinderSeq { run: run_length, pos: y_minus1 + 1, color: last_color });
                last_color = pix;
                run_length = 1;
                if let Some((pos, width)) = seq_buffer.search() {
                    let search_point = Point::new(x as _, pos as _);
                    // log::info!("col candidate {}, {}", x, pos);

                    // now cross the candidate against row candidates; only report those that match
                    for &rc in row_candidates.iter().filter(|&&p| p == search_point) {
                        candidates.push(rc);
                        candidate_width += width;
                    }
                }
            }
        }
    }
    if candidates.len() != 0 { candidate_width / candidates.len() } else { 0 }
}
