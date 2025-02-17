use core::cmp::{max, min};

use crate::minigfx::*;

#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Rectangle {
    pub tl: Point,
    pub br: Point,
    pub style: DrawStyle,
}

impl Rectangle {
    pub fn new(p1: Point, p2: Point) -> Rectangle {
        // always check point ordering
        Rectangle {
            tl: Point::new(min(p1.x, p2.x), min(p1.y, p2.y)),
            br: Point::new(max(p1.x, p2.x), max(p1.y, p2.y)),
            style: DrawStyle::default(),
        }
    }

    pub fn new_coords(x0: isize, y0: isize, x1: isize, y1: isize) -> Rectangle {
        Rectangle {
            tl: Point::new(min(x0, x1), min(y0, y1)),
            br: Point::new(max(x0, x1), max(y0, y1)),
            style: DrawStyle::default(),
        }
    }

    // stack a new rectangle on top of the current one (same width)
    // positive heights go *below*, negative go *above* in screen coordinate space. borders are
    // non-overlapping.
    pub fn new_v_stack(reference: Rectangle, height: isize) -> Rectangle {
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
    pub fn new_h_stack(reference: Rectangle, width: isize) -> Rectangle {
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

    pub fn new_coords_with_style(x0: isize, y0: isize, x1: isize, y1: isize, style: DrawStyle) -> Rectangle {
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
}

//////////////////////// Rounded Rectangle
#[derive(Debug, Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RoundedRectangle {
    pub border: Rectangle, // drawstyle is inherited from the Rectangle
    pub radius: isize,
}
impl RoundedRectangle {
    pub fn new(rr: Rectangle, r: isize) -> RoundedRectangle {
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
