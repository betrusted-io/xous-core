use core::cmp::{max, min};

use crate::api::{ClipRect, DrawStyle, Point};
use crate::op::{HEIGHT, WIDTH};

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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
    // positive heights go *below*, negative go *above* in screen coordinate space. borders are
    // non-overlapping.
    pub fn new_v_stack(reference: Rectangle, height: i16) -> Rectangle {
        if height >= 0 {
            // rectangle below
            Rectangle::new_coords(reference.tl.x, reference.br.y + 1, reference.br.x, reference.br.y + height)
        } else {
            // rectangle above
            Rectangle::new_coords(reference.tl.x, reference.tl.y + height, reference.br.x, reference.tl.y - 1)
        }
    }

    // make a new rectangle than spans between the above and below rectangles. the borders are
    // non-overlapping.
    pub fn new_v_span(above: Rectangle, below: Rectangle) -> Rectangle {
        Rectangle::new_coords(above.tl.x, above.br.y + 1, below.br.x, below.tl.y - 1)
    }

    // "stack" a rectangle to the left or right of the current one (same height)
    // positive widths go to the right, negative to the left. borders are non-overlapping
    pub fn new_h_stack(reference: Rectangle, width: i16) -> Rectangle {
        if width >= 0 {
            // stack to the right
            Rectangle::new_coords(
                reference.br.x + 1,
                reference.tl.y,
                reference.br.x + width + 1,
                reference.br.y,
            )
        } else {
            // stack to the left
            Rectangle::new_coords(
                reference.tl.x + width - 1,
                reference.tl.y,
                reference.tl.x - 1,
                reference.br.y,
            )
        }
    }

    // make a new rectangle than spans between the left and right rectangles. borders are non-overlapping
    pub fn new_h_span(left: Rectangle, right: Rectangle) -> Rectangle {
        Rectangle::new_coords(left.br.x + 1, left.tl.y, right.tl.x - 1, right.br.y)
    }

    pub fn tl(&self) -> Point { self.tl }

    pub fn br(&self) -> Point { self.br }

    pub fn bl(&self) -> Point { Point { x: self.tl.x, y: self.br.y } }

    pub fn tr(&self) -> Point { Point { x: self.br.x, y: self.tl.y } }

    pub fn intersects(&self, other: Rectangle) -> bool {
        !(self.br.x < other.tl.x
            || self.tl.y > other.br.y
            || self.br.y < other.tl.y
            || self.tl.x > other.br.x)
    }

    pub fn intersects_point(&self, point: Point) -> bool {
        ((point.x >= self.tl.x) && (point.x <= self.br.x))
            && ((point.y >= self.tl.y) && (point.y <= self.br.y))
    }

    /// takes the current Rectangle, and clips it with a clipping Rectangle; returns a new rectangle as the
    /// result
    pub fn clip_with(&self, clip: Rectangle) -> Option<Rectangle> {
        // check to see if we even overlap; if not, don't do any computation
        if !self.intersects(clip) {
            return None;
        }
        let tl: Point = Point::new(
            if self.tl.x < clip.tl.x { clip.tl.x } else { self.tl.x },
            if self.tl.y < clip.tl.y { clip.tl.y } else { self.tl.y },
        );
        let br: Point = Point::new(
            if self.br.x > clip.br.x { clip.br.x } else { self.br.x },
            if self.br.y > clip.br.y { clip.br.y } else { self.br.y },
        );
        Some(Rectangle::new(tl, br))
    }

    pub fn new_coords_with_style(x0: i16, y0: i16, x1: i16, y1: i16, style: DrawStyle) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style,
        }
    }

    pub fn new_with_style(p1: Point, p2: Point, style: DrawStyle) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style,
        }
    }

    pub fn x0(&self) -> u32 { self.tl.x as u32 }

    pub fn x1(&self) -> u32 { self.br.x as u32 }

    pub fn y0(&self) -> u32 { self.tl.y as u32 }

    pub fn y1(&self) -> u32 { self.br.y as u32 }

    pub fn width(&self) -> u32 { (self.br.x - self.tl.x) as u32 }

    pub fn height(&self) -> u32 { (self.br.y - self.tl.y) as u32 }

    pub fn translate(&mut self, offset: Point) {
        self.tl.x += offset.x;
        self.br.x += offset.x;
        self.tl.y += offset.y;
        self.br.y += offset.y;
    }

    pub fn translate_chain(self, offset: Point) -> Rectangle {
        Rectangle {
            tl: Point::new(self.tl.x + offset.x, self.tl.y + offset.y),
            br: Point::new(self.br.x + offset.x, self.br.y + offset.y),
            style: self.style,
        }
    }

    pub fn normalize(&mut self) {
        self.br.x -= self.tl.x;
        self.br.y -= self.tl.y;
        self.tl.x = 0;
        self.tl.y = 0;
    }

    // this "margins in" a rectangle on all sides; if the margin is more than the twice any
    // dimension it just reduces the rectangle to a line at the midpoint of the axis dimension
    pub fn margin(&mut self, margin: Point) {
        if margin.x * 2 <= (self.br.x - self.tl.x) {
            self.tl.x += margin.x;
            self.br.x -= margin.x;
        } else {
            let midpoint = (self.br.x + self.tl.x) / 2;
            self.tl.x = midpoint;
            self.br.x = midpoint;
        }
        if margin.y * 2 <= (self.br.y - self.tl.y) {
            self.tl.y += margin.y;
            self.br.y -= margin.y;
        } else {
            let midpoint = (self.br.y + self.tl.y) / 2;
            self.tl.y = midpoint;
            self.br.y = midpoint;
        }
    }

    // this "margins out" on all sides
    pub fn margin_out(&mut self, margin: Point) {
        self.tl.x -= margin.x;
        self.tl.y -= margin.y;
        self.br.x += margin.x;
        self.br.y += margin.y;
    }

    /// Make a rectangle of the full screen size
    pub fn full_screen() -> Rectangle {
        Rectangle { tl: Point::new(0, 0), br: Point::new(WIDTH, HEIGHT), style: DrawStyle::default() }
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

impl From<Rectangle> for ClipRect {
    fn from(rectangle: Rectangle) -> ClipRect {
        ClipRect::new(rectangle.x0() as _, rectangle.y0() as _, rectangle.x1() as _, rectangle.y1() as _)
    }
}

//////////////////////////// LINE

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

//////////////////////////// CIRCLE

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Circle {
    pub center: Point,
    pub radius: i16,

    /// Drawing style
    pub style: DrawStyle,
}

impl Circle {
    pub fn new(c: Point, r: i16) -> Circle { Circle { center: c, radius: r, style: DrawStyle::default() } }

    pub fn new_with_style(c: Point, r: i16, style: DrawStyle) -> Circle {
        Circle { center: c, radius: r, style }
    }

    pub fn translate(&mut self, offset: Point) { self.center = self.center + offset; }
}

//////////////////////// Rounded Rectangle
#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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
        RoundedRectangle { border: rr, radius: r_adj }
    }

    pub fn translate(&mut self, offset: Point) { self.border.translate(offset); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intersection_test() {
        let a = Rectangle::new(Point::new(0, 0), Point::new(100, 150));

        assert!(a.intersects(a));
        // br corner
        assert!(a.intersects(Rectangle::new(Point::new(50, 50), Point::new(200, 200),)));
        // tl corner
        assert!(a.intersects(Rectangle::new(Point::new(-50, -50), Point::new(50, 50),)));
        // tr corner
        assert!(a.intersects(Rectangle::new(Point::new(50, -50), Point::new(250, 50),)));
        // bl corner
        assert!(a.intersects(Rectangle::new(Point::new(-50, 50), Point::new(50, 250),)));
        // enclosed
        assert!(a.intersects(Rectangle::new(Point::new(-50, -50), Point::new(250, 250),)));
        // enclosing
        assert!(a.intersects(Rectangle::new(Point::new(10, 10), Point::new(20, 20),)));
        // left border
        assert!(a.intersects(Rectangle::new(Point::new(-10, 10), Point::new(0, 20),)));
        // right border
        assert!(a.intersects(Rectangle::new(Point::new(100, 10), Point::new(150, 20),)));
        // top border
        assert!(a.intersects(Rectangle::new(Point::new(-100, -10), Point::new(150, 0),)));
        // bottom border
        assert!(a.intersects(Rectangle::new(Point::new(50, 150), Point::new(60, 151),)));
        // within, bordering
        assert!(a.intersects(Rectangle::new(Point::new(0, 20), Point::new(100, 50),)));
        // wider than, from above
        assert!(a.intersects(Rectangle::new(Point::new(-50, -50), Point::new(300, 50))));
        // wider than, from below
        assert!(a.intersects(Rectangle::new(Point::new(-50, 100), Point::new(300, 300))));
        // taller than, from left
        assert!(a.intersects(Rectangle::new(Point::new(-50, -50), Point::new(20, 300))));
        // taller than, from right
        assert!(a.intersects(Rectangle::new(Point::new(20, 50), Point::new(300, 300))));

        // to the left of
        assert!(!a.intersects(Rectangle::new(Point::new(-10, -10), Point::new(-5, -5),)));
        // to the right of
        assert!(!a.intersects(Rectangle::new(Point::new(101, 0), Point::new(150, 150),)));
    }
}
