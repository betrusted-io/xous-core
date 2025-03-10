use core::ops::{Add, AddAssign, Div, DivAssign, Index, Mul, MulAssign, Neg, Sub, SubAssign};

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: isize,
    pub y: isize,
}
impl Point {
    pub fn new(x: isize, y: isize) -> Self { Point { x, y } }

    pub fn to_f32(&self) -> (f32, f32) { (self.x as f32, self.y as f32) }
}

impl Default for Point {
    fn default() -> Self { Point::new(0, 0) }
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

impl Mul<isize> for Point {
    type Output = Point;

    fn mul(self, rhs: isize) -> Point { Point::new(self.x * rhs, self.y * rhs) }
}

impl MulAssign<isize> for Point {
    fn mul_assign(&mut self, rhs: isize) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<isize> for Point {
    type Output = Point;

    fn div(self, rhs: isize) -> Point { Point::new(self.x / rhs, self.y / rhs) }
}

impl DivAssign<isize> for Point {
    fn div_assign(&mut self, rhs: isize) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Index<usize> for Point {
    type Output = isize;

    fn index(&self, idx: usize) -> &isize {
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
    fn from(other: (i16, i16)) -> Self { Point::new(other.0 as isize, other.1 as isize) }
}

impl From<[i16; 2]> for Point {
    fn from(other: [i16; 2]) -> Self { Point::new(other[0] as isize, other[1] as isize) }
}

impl From<&[i16; 2]> for Point {
    fn from(other: &[i16; 2]) -> Self { Point::new(other[0] as isize, other[1] as isize) }
}

impl From<(isize, isize)> for Point {
    fn from(other: (isize, isize)) -> Self { Point::new(other.0 as isize, other.1 as isize) }
}

impl From<[isize; 2]> for Point {
    fn from(other: [isize; 2]) -> Self { Point::new(other[0] as isize, other[1] as isize) }
}

impl From<&[isize; 2]> for Point {
    fn from(other: &[isize; 2]) -> Self { Point::new(other[0] as isize, other[1] as isize) }
}

impl From<Point> for (i16, i16) {
    fn from(other: Point) -> (i16, i16) { (other.x as i16, other.y as i16) }
}

impl From<&Point> for (i16, i16) {
    fn from(other: &Point) -> (i16, i16) { (other.x as i16, other.y as i16) }
}

impl From<Point> for (isize, isize) {
    fn from(other: Point) -> (isize, isize) { (other.x, other.y) }
}

impl From<&Point> for (isize, isize) {
    fn from(other: &Point) -> (isize, isize) { (other.x as isize, other.y as isize) }
}

/// Warning: bounds check on x/y is not implemented to improve performance
impl From<Point> for usize {
    fn from(point: Point) -> usize { (point.x as usize) << 16 | (point.y as usize) }
}

/// Warning: bounds check on x/y is not implemented to improve performance
impl From<usize> for Point {
    fn from(p: usize) -> Point { Point { x: (p >> 16 & 0xffff) as _, y: (p & 0xffff) as _ } }
}
