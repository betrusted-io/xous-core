// dev code for QR code algorithms
use cramium_hal::minigfx::Point;

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
enum Color {
    Black,
    White,
}
impl Color {
    pub fn from(luminance: u8, thresh: u8) -> Self {
        if luminance > thresh { Color::White } else { Color::Black }
    }
}
impl Default for Color {
    fn default() -> Self { Color::Black }
}

const AFF_FP_SHIFT: isize = 16;
const AFF_FP_ONE: f32 = (1 << AFF_FP_SHIFT) as f32;
pub struct AffineTransform {
    pub a: isize,
    pub b: isize,
    pub c: isize,
    pub d: isize,
    pub tx: isize,
    pub ty: isize,
    pub rows: isize,
    pub cols: isize,
}

impl AffineTransform {
    pub fn from_coordinates(tl: Point, tr: Point, cols: usize, rows: usize) -> Self {
        // crate::println!("{}/{}", tr.y - tl.y, tr.x - tl.x);
        let angle: f32 = libm::atan2f(tr.y as f32 - tl.y as f32, tr.x as f32 - tl.x as f32);
        // crate::println!("angle: {}", angle);
        Self {
            a: (libm::cosf(angle) * AFF_FP_ONE) as isize,
            b: (-libm::sinf(angle) * AFF_FP_ONE) as isize,
            c: (libm::sinf(angle) * AFF_FP_ONE) as isize,
            d: (libm::cosf(angle) * AFF_FP_ONE) as isize,
            tx: 0,
            ty: 0,
            cols: cols as isize,
            rows: rows as isize,
        }
    }

    pub fn transform(&self, src: &[u8], dst: &mut [core::mem::MaybeUninit<u8>]) {
        for y in 0..self.rows as usize {
            for x in 0..self.cols as usize {
                let x_src = (self.a * x as isize + self.b * y as isize + self.tx) >> AFF_FP_SHIFT;
                let y_src = (self.c * x as isize + self.d * y as isize + self.ty) >> AFF_FP_SHIFT;
                dst[y * self.cols as usize + x] = core::mem::MaybeUninit::new(
                    if x_src >= 0 && x_src < self.cols && y_src >= 0 && y_src < self.rows {
                        src[x_src as usize + y_src as usize * self.cols as usize]
                    } else {
                        0
                    },
                );
            }
        }
    }
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

    pub fn search(&self) -> Option<usize> {
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
                    // crate::println!("  seq {:?}", &seq);
                    // crate::println!("  ratios {:?}", &ratios);
                    Some(seq[2].pos - seq[2].run / 2 - 1)
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

const ROW_LIMIT: usize = 128;
pub fn find_finders(candidates: &mut [Option<Point>], image: &[u8], thresh: u8, stride: usize) {
    let mut row_candidates: [Option<Point>; ROW_LIMIT] = [None; ROW_LIMIT];

    // ideally, the candidates would be a Vec, but we want this to work without allocations
    // so we're going to insert them manually into a list.
    let mut row_candidate_index = 0;

    let mut seq_buffer = SeqBuffer::new();

    // crate::println!("ROWSROWSROWSROWS");
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
                if let Some(pos) = seq_buffer.search() {
                    // crate::println!("row candidate {}, {}", pos, y);
                    row_candidates[row_candidate_index] = Some(Point::new(pos as _, y as _));
                    row_candidate_index += 1;
                    if row_candidate_index == row_candidates.len() {
                        // just abort the search if we run out of space to store results
                        break;
                    }
                }
            }
        }
        if row_candidate_index == row_candidates.len() {
            crate::println!("ran out of space for row candidates");
            break;
        }
    }

    // crate::println!("CCCCCCCCCCCCCCCC");
    let mut candidate_index = 0;
    for x in 0..stride {
        seq_buffer.clear();

        let mut last_color = Color::from(image[x], thresh);
        let mut run_length = 1;
        for (y_minus1, row) in image[x + stride..].chunks(stride).enumerate() {
            let pix = Color::from(row[0], thresh);
            if pix == last_color {
                run_length += 1;
            } else {
                seq_buffer.push(FinderSeq { run: run_length, pos: y_minus1 + 1, color: last_color });
                last_color = pix;
                run_length = 1;
                if let Some(pos) = seq_buffer.search() {
                    let search_point = Point::new(x as _, pos as _);
                    // crate::println!("col candidate {}, {}", x, pos);

                    // now cross the candidate against row candidates; only report those that match
                    for &rc in row_candidates
                        .iter()
                        .filter(|&&x| if let Some(p) = x { p == search_point } else { false })
                    {
                        if candidate_index < candidates.len() {
                            candidates[candidate_index] = rc;
                            candidate_index += 1;
                        } else {
                            // just abort the search if we run out of space to store results
                            break;
                        }
                    }
                }
            }
            if candidate_index == candidates.len() {
                // just abort the search if we run out of space to store results
                crate::println!("ran out of space for processed candidates");
                break;
            }
        }
    }
}
