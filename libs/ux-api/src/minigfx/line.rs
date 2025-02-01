use crate::minigfx::*;

#[derive(Debug, Clone, Copy)]
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
