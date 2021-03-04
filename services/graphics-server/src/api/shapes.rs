use crate::op::{HEIGHT, WIDTH};
use crate::api::{Point, DrawStyle};
use blitstr_ref as blitstr;
use blitstr::{ClipRect};
use core::{cmp::{max, min}};

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Unarchive)]
pub struct Rectangle {
    /// Top left point of the rect
    pub tl: Point,

    /// Bottom right point of the rect
    pub br: Point,

    /// Drawing style
    pub style: DrawStyle,
}

//////////////////////////// RECTANGLE

impl Rectangle {
    pub fn new(p1: Point, p2: Point) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style: DrawStyle::default(),
        }
    }
    pub fn new_coords(x0: i16, y0: i16, x1: i16, y1: i16) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: DrawStyle::default(),
        }
    }
    // stack a new rectangle on top of the current one (same width)
    // positive heights go *below*, negative go *above* in screen coordinate space. borders are non-overlapping.
    pub fn new_v_stack(reference: Rectangle, height: i16) -> Rectangle {
        if height >= 0 { // rectangle below
            Rectangle::new_coords(reference.tl.x, reference.br.y + 1,
            reference.br.x, reference.br.y + height)
        } else { // rectangle above
            Rectangle::new_coords(reference.tl.x, reference.tl.y + height,
            reference.br.x, reference.tl.y - 1)
        }
    }
    // make a new rectangle than spans between the above and below rectangles. the borders are non-overlapping.
    pub fn new_v_span(above: Rectangle, below: Rectangle) -> Rectangle {
        Rectangle::new_coords(above.tl.x, above.br.y + 1, below.br.x, below.tl.y - 1)
    }
    // "stack" a rectangle to the left or right of the current one (same height)
    // positive widths go to the right, negative to the left. borders are non-overlapping
    pub fn new_h_stack(reference: Rectangle, width: i16) -> Rectangle {
        if width >= 0 { // stack to the right
            Rectangle::new_coords(reference.br.x + 1, reference.tl.y,
            reference.br.x + width + 1, reference.br.y)
        } else { // stack to the left
            Rectangle::new_coords(reference.tl.x + width - 1, reference.tl.y,
            reference.tl.x - 1, reference.br.y)
        }
    }
    // make a new rectangle than spans between the left and right rectangles. borders are non-overlapping
    pub fn new_h_span(left: Rectangle, right: Rectangle) -> Rectangle {
        Rectangle::new_coords(left.br.x + 1, left.tl.y, right.tl.x - 1, right.br.y)
    }
    // other: 0,0/336,536
    // self: 0,476/336,505
    pub fn intersects(&self, other: Rectangle) -> bool {
        ((other.tl.x >= self.tl.x) && (other.tl.x <= self.br.x)) &&
        ((other.tl.y >= self.tl.y) && (other.tl.y <= self.br.y))
        ||
        ((other.br.x >= self.tl.x) && (other.br.x <= self.br.x)) &&
        ((other.br.y >= self.tl.y) && (other.br.y <= self.br.y))
        ||
        // case that self is inside other
        ((self.tl.x >= other.tl.x) && (self.tl.x <= other.br.x)) &&
        ((self.tl.y >= other.tl.y) && (self.tl.y <= other.br.y))
        ||
        ((self.br.x >= other.tl.x) && (self.br.x <= other.br.x)) &&
        ((self.br.y >= other.tl.y) && (self.br.y <= other.br.y))
    }
    pub fn intersects_point(&self, point: Point) -> bool {
        ((point.x >= self.tl.x) && (point.x <= self.br.x)) &&
        ((point.y >= self.tl.y) && (point.y <= self.br.y))
    }
    pub fn new_coords_with_style(
        x0: i16,
        y0: i16,
        x1: i16,
        y1: i16,
        style: DrawStyle,
    ) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: style,
        }
    }
    pub fn new_with_style(p1: Point, p2: Point, style: DrawStyle) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style: style,
        }
    }
    pub fn x0(&self) -> u32 {
        self.tl.x as u32
    }
    pub fn x1(&self) -> u32 {
        self.br.x as u32
    }
    pub fn y0(&self) -> u32 {
        self.tl.y as u32
    }
    pub fn y1(&self) -> u32 {
        self.br.y as u32
    }
    pub fn translate(&mut self, offset: Point) {
        self.tl.x += offset.x;
        self.br.x += offset.x;
        self.tl.y += offset.y;
        self.br.y += offset.y;
    }
    pub fn normalize(&mut self) {
        self.br.x -= self.tl.x;
        self.br.y -= self.tl.y;
        self.tl.x = 0;
        self.tl.y = 0;
    }

    /// Make a rectangle of the full screen size
    pub fn full_screen() -> Rectangle {
        Rectangle {
            tl: Point::new(0, 0),
            br: Point::new(WIDTH, HEIGHT),
            style: DrawStyle::default(),
        }
    }
    /// Make a rectangle of the screen size minus padding
    pub fn padded_screen() -> Rectangle {
        let pad = 6;
        Rectangle {
            tl: Point::new(pad, pad),
            br: Point::new(WIDTH - pad, HEIGHT - pad),
            style: DrawStyle::default(),
        }
    }
}

impl Into<ClipRect> for Rectangle {
    fn into(self) -> ClipRect {
        ClipRect::new(self.x0() as u32, self.y0() as u32, self.x1() as u32, self.y1() as u32)
    }
}


//////////////////////////// LINE

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Unarchive)]
pub struct Line {
    pub start: Point,
    pub end: Point,

    /// Drawing style
    pub style: DrawStyle,
}

impl Line {
    pub fn new(start: Point, end: Point) -> Line {
        Line {
            start: start,
            end: end,
            style: DrawStyle::default(),
        }
    }
    pub fn new_with_style(start: Point, end: Point, style: DrawStyle) -> Line {
        Line {
            start: start,
            end: end,
            style: style,
        }
    }
}


//////////////////////////// CIRCLE

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Unarchive)]
pub struct Circle {
    pub center: Point,
    pub radius: i16,

    /// Drawing style
    pub style: DrawStyle,
}

impl Circle {
    pub fn new(c: Point, r: i16) -> Circle {
        Circle {
            center: c,
            radius: r,
            style: DrawStyle::default(),
        }
    }
    pub fn new_with_style(c: Point, r: i16, style: DrawStyle) -> Circle {
        Circle {
            center: c,
            radius: r,
            style,
        }
    }
}

//////////////////////// Rounded Rectangle
#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Unarchive)]
pub struct RoundedRectangle {
    pub border: Rectangle, // drawstyle is inherited from the Rectangle
    pub radius: i16,
}
impl RoundedRectangle {
    pub fn new(rr: Rectangle, r: i16) -> RoundedRectangle {
        let mut r_adj = r;
        // disallow radii that are greater than 2x the width of the rectangle
        if r > (rr.br.x - rr.tl.x) {
            r_adj = 0;
        }
        if r > (rr.br.y - rr.tl.y) {
            r_adj = 0;
        }
        RoundedRectangle {
            border: rr,
            radius: r_adj,
        }
    }
}
