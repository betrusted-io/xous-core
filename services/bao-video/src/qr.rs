use core::cell::RefCell;
use core::convert::TryFrom;
use core::ops::{BitXor, Not};

use ux_api::minigfx::{ColorNative, FrameBuffer, Point};

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

/// Finder search margin, as defined by expected QR code code widths (so this scales with the effective
/// resolution of the code)
pub const FINDER_SEARCH_MARGIN: isize = 2;
/// How much we want the final QR image to be "pulled in" from the outer edge of the image buffer
pub const HOMOGRAPHY_MARGIN: isize = -4;
pub const CROSSHAIR_LEN: isize = 3;

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

#[allow(dead_code)]
/// Used for debugging QR code algorithms
pub fn draw_line(image: &mut dyn FrameBuffer, l: &LineDerivation, color: ColorNative) {
    let axis = l.independent_axis;
    let (m, b) = l.equation.unwrap();
    match axis {
        Axis::X => {
            for x in 0..image.dimensions().x {
                let y = (x as f32 * m + b) as isize;
                if y >= 0 && y < image.dimensions().y as isize {
                    image.put_pixel(Point::new(x, y), color);
                }
            }
        }
        Axis::Y => {
            for y in 0..image.dimensions().y {
                let x = (y as f32 * m + b) as isize;
                if x >= 0 && x < image.dimensions().x as isize {
                    image.put_pixel(Point::new(x, y), color);
                }
            }
        }
    }
}
struct DirRange {
    start: usize,
    stop_exclusive: usize,
    is_up: bool,
    range: DirectionalRange,
}
impl Iterator for DirRange {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> { self.range.next() }
}
impl DirRange {
    pub fn new(start: usize, stop_exclusive: usize, is_up: bool) -> Self {
        Self {
            start,
            stop_exclusive,
            is_up,
            range: if is_up {
                DirectionalRange::Up(start..stop_exclusive)
            } else {
                DirectionalRange::Down((start..stop_exclusive).rev())
            },
        }
    }

    pub fn reset(&mut self) {
        self.range = if self.is_up {
            DirectionalRange::Up(self.start..self.stop_exclusive)
        } else {
            DirectionalRange::Down((self.start..self.stop_exclusive).rev())
        }
    }
}

enum DirectionalRange {
    Up(core::ops::Range<usize>),
    Down(core::iter::Rev<core::ops::Range<usize>>),
}

impl Iterator for DirectionalRange {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            DirectionalRange::Up(iter) => iter.next(),
            DirectionalRange::Down(iter) => iter.next(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

fn adjust_axis(p: Point, axis: Axis) -> (f32, f32) {
    (
        match axis {
            Axis::X => p.x as f32,
            Axis::Y => p.y as f32,
        },
        match axis {
            Axis::X => p.y as f32,
            Axis::Y => p.x as f32,
        },
    )
}

fn least_squares_fit(points: &[Point], axis: Axis) -> (f32, f32) {
    let n = points.len() as f32;
    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    let mut sum_xx = 0.0f32;
    let mut sum_xy = 0.0f32;
    for &point in points.iter() {
        // flip x/y coordinates based on the independent axis
        let (x, y) = adjust_axis(point, axis);
        sum_x += x;
        sum_y += y;
        sum_xx += x * x;
        sum_xy += x * y;
    }
    let m = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
    let b = (sum_y - m * sum_x) / n;
    (m, b)
}

fn point_from_hv_lines(hline: &LineDerivation, vline: &LineDerivation) -> Option<Point> {
    if let Some((m1, b1)) = hline.equation {
        if let Some((m2v, b2v)) = vline.equation {
            log::info!("h: {}, {} | v: {}, {}", m1, b1, m2v, b2v);
            if m2v != 0.0 {
                let m2 = 1.0 / m2v;
                let b2 = -b2v / m2v;
                let x = (b2 - b1) / (m1 - m2);
                let y = m1 * x + b1;
                Some(Point::new(x as isize, y as isize))
            } else {
                let y = m1 * b2v + b1;
                Some(Point::new(b2v as isize, y as isize))
            }
        } else {
            None
        }
    } else {
        None
    }
}

// Threshold to reject points if they don't fit on the best-fit line
const OUTLIER_THRESHOLD: f32 = 0.5;
const OUTLIER_ITERS: usize = 5;
#[derive(Clone)]
pub struct LineDerivation {
    pub equation: Option<(f32, f32)>,
    pub independent_axis: Axis,
    pub data_points: Vec<Point>,
}
impl LineDerivation {
    pub fn new(axis: Axis) -> Self {
        LineDerivation { equation: None, independent_axis: axis, data_points: Vec::new() }
    }

    pub fn push(&mut self, p: Point) { self.data_points.push(p); }

    /// This implementation heavily relies on f32, so it is slow on an embedded processor; however,
    /// we need the precision and the solving should be done only rarely.
    pub fn solve(&mut self) {
        let mut m_guess: f32 = 0.0;
        let mut b_guess: f32 = 0.0;
        let mut converged_in = 0;
        let mut points = self.data_points.clone();
        for guesses in 0..OUTLIER_ITERS {
            let mut filtered_points = Vec::<Point>::new();
            let mut residuals = Vec::<f32>::new();
            converged_in = guesses;
            // guess a best-fit line
            (m_guess, b_guess) = least_squares_fit(&points, self.independent_axis);
            // compute the residuals of the points to the guessed line
            for &p in points.iter() {
                let (x, y) = adjust_axis(p, self.independent_axis);
                let predicted_y = m_guess * x + b_guess;
                residuals.push((y - predicted_y).abs());
            }
            // extract the median residual
            let mut sorted_residuals = residuals.clone();
            sorted_residuals
                .sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Greater));
            let threshold = (sorted_residuals[points.len() / 2] * 1.5).max(OUTLIER_THRESHOLD);

            for (i, &p) in points.iter().enumerate() {
                if residuals[i] <= threshold {
                    filtered_points.push(p);
                }
            }
            if filtered_points.len() == points.len() {
                break;
            } else {
                points = filtered_points;
            }
        }
        log::info!("Solver converged in {} iterations", converged_in);
        self.equation = Some((m_guess, b_guess))
    }
}

pub struct Corner {
    pub finder_ref: Option<Point>,
    pub h_line: LineDerivation,
    pub v_line: LineDerivation,
}
impl Default for Corner {
    fn default() -> Self {
        Self { finder_ref: None, h_line: LineDerivation::new(Axis::X), v_line: LineDerivation::new(Axis::Y) }
    }
}

#[derive(Default)]
pub struct QrCorners {
    // Relies on the property that Direction enumerates and iterates consistently as a usize from 0-3,
    // which covers each of the four corner directions exactly and uniquely.
    corners: [Corner; 4],
    width: isize,
    height: isize,
    derived_corner: Direction,
    finder_width: usize,
    // destination QR image in pixels per side (as a perfect square)
    dst_pixels: usize,
}
impl QrCorners {
    pub fn from_finders(points: &[Point; 3], dimensions: Point, finder_width: usize) -> Option<Self> {
        let x_half = dimensions.x / 2;
        let y_half = dimensions.y / 2;

        let mut qrc = QrCorners::default();
        qrc.width = dimensions.x;
        qrc.height = dimensions.y;
        qrc.finder_width = finder_width;
        qrc.dst_pixels = dimensions.x.min(dimensions.y) as usize;

        for &p in points {
            if p.x < x_half && p.y < y_half {
                qrc.corners[Direction::SouthWest as usize].finder_ref = Some(p);
            } else if p.x < x_half && p.y >= y_half {
                qrc.corners[Direction::NorthWest as usize].finder_ref = Some(p);
            } else if p.x >= x_half && p.y < y_half {
                qrc.corners[Direction::SouthEast as usize].finder_ref = Some(p);
            } else if p.x >= x_half && p.y >= y_half {
                qrc.corners[Direction::NorthEast as usize].finder_ref = Some(p);
            }
        }

        // check that at least three corners are filled
        if qrc.corners.iter().map(|c| if c.finder_ref.is_some() { 1 } else { 0 }).sum::<usize>() == 3 {
            for (dir_index, corner) in qrc.corners.iter().enumerate() {
                if corner.finder_ref.is_none() {
                    qrc.derived_corner = Direction::try_from(dir_index).unwrap();
                }
            }
            Some(qrc)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn derived_corner(&self) -> Direction { self.derived_corner }

    #[allow(dead_code)]
    pub fn center_point(&self, dir: Direction) -> Option<Point> { self.corners[dir as usize].finder_ref }

    fn outline_search(&mut self, ir: &mut ImageRoi) {
        for (d, corner) in self.corners.iter_mut().enumerate() {
            let direction = Direction::try_from(d).unwrap();
            // this test ensures we automatically skip the "missing" corner
            if let Some(p) = corner.finder_ref {
                ir.set_roi(
                    Point::new(
                        (p.x - self.finder_width as isize / 2).max(0),
                        (p.y + self.finder_width as isize / 2).min(ir.height as isize),
                    ),
                    Point::new(
                        (p.x + self.finder_width as isize / 2).min(ir.width as isize),
                        (p.y - self.finder_width as isize / 2).max(0),
                    ),
                );
                let signs: Point = direction.into();

                let mut y_range = if signs.y < 0 {
                    DirRange::new(0, ir.roi_height(), true)
                } else {
                    DirRange::new(0, ir.roi_height(), false)
                };
                let mut x_range = if signs.x < 0 {
                    DirRange::new(0, ir.roi_width(), true)
                } else {
                    DirRange::new(0, ir.roi_width(), false)
                };

                loop {
                    if let Some(y) = y_range.next() {
                        x_range.reset();
                        loop {
                            if let Some(x) = x_range.next() {
                                if Color::Black
                                    == ir.get_roi_binary_pixel(Point::new(x as isize, y as isize)).unwrap()
                                {
                                    corner.v_line.push(
                                        ir.roi_to_absolute(Point::new(x as isize, y as isize)).unwrap(),
                                    );
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
                corner.v_line.solve();

                y_range.reset();
                x_range.reset();

                loop {
                    if let Some(x) = x_range.next() {
                        y_range.reset();
                        loop {
                            if let Some(y) = y_range.next() {
                                if Color::Black
                                    == ir.get_roi_binary_pixel(Point::new(x as isize, y as isize)).unwrap()
                                {
                                    corner.h_line.push(
                                        ir.roi_to_absolute(Point::new(x as isize, y as isize)).unwrap(),
                                    );
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
                corner.h_line.solve();
            }
        }
    }

    /// Returns a (src, dst) tuple of point mappings for homomorphic transformation.
    /// The destination is the four corners of image shape as specified when the structure
    /// was made, with a margin added.
    ///
    /// The margin should be negative for the corners to go in toward the center.
    ///
    /// Canonical orientation for QR code should be like this:
    ///   +------+
    ///   |O    .|
    ///   |      |
    /// ^ |O    O|
    /// | +______+
    /// (0,0) ->
    ///
    /// Thus we want to orient so the discovered corner is in the NorthEast direction.
    pub fn mapping(&mut self, ir: &mut ImageRoi, margin: isize) -> ([Option<Point>; 4], [Option<Point>; 4]) {
        // adjust the final target image size by the specified margin
        self.dst_pixels = (self.dst_pixels as isize + 2 * margin) as usize;

        // first, search for the lines that define the outline of the QR code
        self.outline_search(ir);

        let mut src = [None; 4];
        let mut dst = [None; 4];
        let mut uk_direction: Option<Direction> = None;

        for (i, corner) in self.corners.iter().enumerate() {
            if let Some(_p) = corner.finder_ref {
                // This is a known finder corner:
                // derive the corner from the extracted h and v lines along the finder pattern
                src[i] = point_from_hv_lines(&corner.h_line, &corner.v_line);
            } else {
                uk_direction = Direction::try_from(i).ok();
                // This is the unknown corner:
                // derive the corner from the h and v lines from the nearest finders' lines
                let h_line = match Direction::try_from(i) {
                    Ok(Direction::NorthWest) => &self.corners[Direction::NorthEast as usize].h_line,
                    Ok(Direction::NorthEast) => &self.corners[Direction::NorthWest as usize].h_line,
                    Ok(Direction::SouthWest) => &self.corners[Direction::SouthEast as usize].h_line,
                    Ok(Direction::SouthEast) => &self.corners[Direction::SouthWest as usize].h_line,
                    _ => panic!("Bad index"),
                };
                let v_line = match Direction::try_from(i) {
                    Ok(Direction::NorthWest) => &self.corners[Direction::SouthWest as usize].v_line,
                    Ok(Direction::NorthEast) => &self.corners[Direction::SouthEast as usize].v_line,
                    Ok(Direction::SouthWest) => &self.corners[Direction::NorthWest as usize].v_line,
                    Ok(Direction::SouthEast) => &self.corners[Direction::NorthEast as usize].v_line,
                    _ => panic!("Bad index"),
                };
                // draw_line(debug, &h_line, [255, 255, 0]);
                // draw_line(debug, &v_line, [255, 255, 0]);
                src[i] = point_from_hv_lines(h_line, v_line);
            }
            let min = -margin;
            let max = self.dst_pixels as isize + margin;
            dst[i] = match Direction::try_from(i) {
                Ok(Direction::NorthWest) => Some(Point::new(min, max)),
                Ok(Direction::NorthEast) => Some(Point::new(max, max)),
                Ok(Direction::SouthWest) => Some(Point::new(min, min)),
                Ok(Direction::SouthEast) => Some(Point::new(max, min)),
                _ => None,
            };
        }

        // now shuffle the dst points such that the unknown corner is in the
        // NorthEast direction
        let mut shuffle: [Option<Point>; 4] = [None; 4];
        shuffle.copy_from_slice(&dst);
        if let Some(uk_corner) = uk_direction {
            let corner_index: usize = uk_corner.into();
            if corner_index != 0 {
                for _ in 0..corner_index {
                    shuffle[1..].copy_from_slice(&dst[..3]);
                    shuffle[0] = dst[3];
                    dst.copy_from_slice(&shuffle);
                }
            }
        }
        (src, shuffle)
    }

    pub fn qr_pixels(&self) -> usize { self.dst_pixels }
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

#[derive(Copy, Clone, Debug)]
/// We use Direction as both a way to encode a meaning and a unique array index.
#[repr(usize)]
pub enum Direction {
    NorthWest = 0,
    NorthEast = 1,
    SouthWest = 2,
    SouthEast = 3,
    North = 4,
    West = 5,
    East = 6,
    South = 7,
}
impl Default for Direction {
    fn default() -> Self { Self::NorthWest }
}
impl Into<Point> for Direction {
    fn into(self) -> Point {
        use Direction::*;
        match self {
            North => Point::new(0, 1),
            West => Point::new(-1, 0),
            East => Point::new(1, 0),
            South => Point::new(0, -1),
            NorthWest => Point::new(-1, 1),
            NorthEast => Point::new(1, 1),
            SouthWest => Point::new(-1, -1),
            SouthEast => Point::new(1, -1),
        }
    }
}
impl Into<usize> for Direction {
    fn into(self) -> usize {
        use Direction::*;
        match self {
            NorthWest => 0,
            NorthEast => 1,
            SouthWest => 2,
            SouthEast => 3,
            North => 4,
            West => 5,
            East => 6,
            South => 7,
        }
    }
}
impl TryFrom<usize> for Direction {
    type Error = &'static str;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        use Direction::*;
        match value {
            0 => Ok(NorthWest),
            1 => Ok(NorthEast),
            2 => Ok(SouthWest),
            3 => Ok(SouthEast),
            4 => Ok(North),
            5 => Ok(West),
            6 => Ok(East),
            7 => Ok(South),
            _ => Err("Invalid direction coding"),
        }
    }
}
impl Direction {
    pub fn eight_way_iter() -> EightConnectedIter { EightConnectedIter { index: 0 } }
}
pub struct EightConnectedIter {
    index: usize,
}

impl Iterator for EightConnectedIter {
    type Item = Direction;

    fn next(&mut self) -> Option<Self::Item> {
        let direction = match self.index {
            0 => Some(Direction::NorthWest),
            1 => Some(Direction::NorthEast),
            2 => Some(Direction::SouthWest),
            3 => Some(Direction::SouthEast),
            4 => Some(Direction::North),
            5 => Some(Direction::West),
            6 => Some(Direction::East),
            7 => Some(Direction::South),
            _ => None,
        };
        self.index += 1;
        direction
    }
}

/// (0, 0) is at the lower left corner
pub struct ImageRoi<'a> {
    pub data: &'a mut [u8],
    pub width: usize,
    pub height: usize,
    thresh: u8,
    // coordinates of a subimage, if set. The ROI includes these points.
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
    iter_row: RefCell<usize>,
    col_iter_start: usize,
    col_iter_row_index: RefCell<usize>,
    iter_col: RefCell<usize>,
}
impl<'a> ImageRoi<'a> {
    pub fn new(data: &'a mut [u8], dimensions: Point, thresh: u8) -> Self {
        // ROI is default the entire area
        Self {
            data,
            width: dimensions.x as usize,
            height: dimensions.y as usize,
            thresh,
            x0: 0,
            x1: dimensions.x as usize,
            y0: 0,
            y1: dimensions.y as usize,
            iter_row: RefCell::new(0),
            col_iter_row_index: RefCell::new(0),
            col_iter_start: 0,
            iter_col: RefCell::new(0),
        }
    }

    pub fn binarize(&self, luma: u8) -> Color { if luma > self.thresh { Color::White } else { Color::Black } }

    pub fn get_pixel(&self, x: usize, y: usize) -> u8 { self.data[x + y * self.width] }

    pub fn get_roi_binary_pixel(&self, roi_point: Point) -> Option<Color> {
        if let Some(abs_p) = self.roi_to_absolute(roi_point) {
            let p = self.get_pixel(abs_p.x as usize, abs_p.y as usize);
            Some(self.binarize(p))
        } else {
            None
        }
    }

    pub fn set_roi(&mut self, tl: Point, br: Point) {
        assert!(tl.x >= 0);
        assert!(tl.y >= 0);
        assert!(br.x >= 0);
        assert!(br.y >= 0);
        assert!(br.x >= tl.x);
        assert!(tl.y >= br.y);
        self.x0 = tl.x as usize;
        self.x1 = br.x as usize;
        self.y0 = br.y as usize;
        self.y1 = tl.y as usize;
        *self.iter_row.borrow_mut() = 0;
        *self.iter_col.borrow_mut() = 0;
        *self.col_iter_row_index.borrow_mut() = 0;
        self.col_iter_start = 0;
    }

    pub fn roi_width(&self) -> usize { self.x1 - self.x0 }

    pub fn roi_height(&self) -> usize { self.y1 - self.y0 }

    pub fn roi_to_absolute(&self, point: Point) -> Option<Point> {
        let x = point.x + self.x0 as isize;
        let y = point.y + self.y0 as isize;
        if x >= 0 && x < self.width as isize && y >= 0 && y < self.height as isize {
            Some(Point::new(x, y))
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn absolute_to_roi(&self, point: Point) -> Option<Point> {
        let x = point.x - self.x0 as isize;
        let y = point.y - self.y0 as isize;
        if x >= 0 && x < self.x1 as isize && y >= 0 && y < self.y1 as isize {
            Some(Point::new(x, y))
        } else {
            None
        }
    }

    pub fn neighbor_luma(&self, point: Point) -> Option<u8> {
        let x = point.x;
        let y = point.y;
        let mut accumulated_luma = 0;
        let mut total_counted = 0;
        if x >= 0 && x < self.width as isize && y >= 0 && y < self.height as isize {
            for direction in Direction::eight_way_iter() {
                total_counted += 1;
                let d: Point = direction.into();
                let p: Point = d + point;
                accumulated_luma += self.data[(p.y * (self.width as isize) + p.x).max(0) as usize] as usize;
            }
        }
        if total_counted > 0 { Some((accumulated_luma / total_counted) as u8) } else { None }
    }
}

impl<'a> Iterator for ImageRoi<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        let cur_row = *self.col_iter_row_index.borrow();
        if cur_row < self.y1 - self.y0 {
            *self.col_iter_row_index.borrow_mut() = cur_row + 1;
            Some(self.data[(self.y0 + cur_row) * self.width + self.x0 + self.col_iter_start])
        } else {
            None
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

const MODULES_BY_VERSION: [usize; 40] = [
    21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65, 69, 73, 77, 81, 85, 89, 93, 97, 101, 105, 109, 113, 117,
    121, 125, 129, 133, 137, 141, 145, 149, 153, 157, 161, 165, 169, 173, 177,
];
const FP_SHIFT: usize = 1 << 16;
/// Returns tuple of (version, expected_modules)
pub fn guess_code_version(finder_width_pix: usize, qr_width_pix: usize) -> (usize, usize) {
    let incoming_ratio = ((qr_width_pix * FP_SHIFT) / finder_width_pix) as isize;
    let mut candidate_ratio = usize::MAX;
    let mut candidate = 0;
    let mut returned_modules = 0;
    for (i, &modules) in MODULES_BY_VERSION.iter().enumerate() {
        let version_ratio = ((modules * FP_SHIFT) / 7) as isize;
        log::debug!(
            "{}: version_ratio {} incoming_ratio {} diff {}",
            i + 1,
            version_ratio,
            incoming_ratio,
            version_ratio - incoming_ratio
        );
        let prev_candidate = candidate_ratio;
        candidate_ratio = candidate_ratio.min((version_ratio - incoming_ratio).abs() as usize);
        if prev_candidate != candidate_ratio {
            candidate = i + 1;
            returned_modules = modules;
        }
    }
    (candidate, returned_modules)
}
