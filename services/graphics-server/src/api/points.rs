use core::ops::{Add, AddAssign, Index, Neg, Sub, SubAssign};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub fn new(x: i16, y: i16) -> Point { Point { x, y } }

    /// Creates a point with X and Y equal to zero.
    pub const fn zero() -> Self { Point { x: 0, y: 0 } }
}

impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point { Point::new(self.x + other.x, self.y + other.y) }
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point { Point::new(self.x - other.x, self.y - other.y) }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Point) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl Index<usize> for Point {
    type Output = i16;

    fn index(&self, idx: usize) -> &i16 {
        match idx {
            0 => &self.x,
            1 => &self.y,
            _ => panic!("index out of bounds: the len is 2 but the index is {}", idx),
        }
    }
}

impl Neg for Point {
    type Output = Point;

    fn neg(self) -> Self::Output { Point::new(-self.x, -self.y) }
}

impl From<(i16, i16)> for Point {
    fn from(other: (i16, i16)) -> Self { Point::new(other.0, other.1) }
}

impl From<[i16; 2]> for Point {
    fn from(other: [i16; 2]) -> Self { Point::new(other[0], other[1]) }
}

impl From<&[i16; 2]> for Point {
    fn from(other: &[i16; 2]) -> Self { Point::new(other[0], other[1]) }
}

impl From<Point> for (i16, i16) {
    fn from(other: Point) -> (i16, i16) { (other.x, other.y) }
}

impl From<&Point> for (i16, i16) {
    fn from(other: &Point) -> (i16, i16) { (other.x, other.y) }
}

impl From<Point> for usize {
    fn from(point: Point) -> usize { (point.x as usize) << 16 | (point.y as usize) }
}

impl From<usize> for Point {
    fn from(p: usize) -> Point { Point { x: (p >> 16 & 0xffff) as _, y: (p & 0xffff) as _ } }
}
