use crate::minigfx::*;

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct Line {
    pub start: Point,
    pub end: Point,

    /// Drawing style
    pub style: DrawStyle,
}

impl Line {
    pub fn new(start: Point, end: Point) -> Line { Line { start, end, style: DrawStyle::default() } }

    pub fn new_with_style(start: Point, end: Point, style: DrawStyle) -> Line { Line { start, end, style } }

    pub fn translate(&mut self, offset: Point) {
        self.start = self.start + offset;
        self.end = self.end + offset;
    }
}
