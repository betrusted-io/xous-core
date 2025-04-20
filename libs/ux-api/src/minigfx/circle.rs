use crate::minigfx::*;

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct Circle {
    pub center: Point,
    pub radius: isize,

    /// Drawing style
    pub style: DrawStyle,
}

impl Circle {
    pub fn new(c: Point, r: isize) -> Circle { Circle { center: c, radius: r, style: DrawStyle::default() } }

    pub fn new_with_style(c: Point, r: isize, style: DrawStyle) -> Circle {
        Circle { center: c, radius: r, style }
    }

    pub fn translate(&mut self, offset: Point) { self.center = self.center + offset; }
}
