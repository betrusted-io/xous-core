use crate::minigfx::Point;

/// Type wrapper for native colors
#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorNative(pub usize);
impl From<usize> for ColorNative {
    fn from(value: usize) -> Self { Self { 0: value } }
}
impl Into<usize> for ColorNative {
    fn into(self) -> usize { self.0 }
}
impl From<PixelColor> for ColorNative {
    fn from(value: PixelColor) -> Self { Self { 0: value.into() } }
}
impl Into<PixelColor> for ColorNative {
    fn into(self) -> PixelColor { PixelColor::from(self.0) }
}

/// Style properties for an object
#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<PixelColor>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<PixelColor>,

    /// Stroke width
    pub stroke_width: isize,
}

impl DrawStyle {
    pub fn new(fill: PixelColor, stroke: PixelColor, width: isize) -> Self {
        Self { fill_color: Some(fill), stroke_color: Some(stroke), stroke_width: width }
    }

    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: PixelColor) -> Self {
        Self { stroke_color: Some(stroke_color), ..DrawStyle::default() }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self { fill_color: Some(PixelColor::Dark), stroke_color: Some(PixelColor::Dark), stroke_width: 1 }
    }
}

impl From<usize> for DrawStyle {
    fn from(s: usize) -> Self {
        // usize split into these words:
        //  31 ...  16  15 ... 4     3..2    1..0
        //    width       rsvd      stroke   fill
        // where the MSB of stroke/fill encodes Some/None
        let fc: ColorNative = (s & 0b00_01).into();
        let sc: ColorNative = (s & 0b01_00).into();
        DrawStyle {
            fill_color: if s & 0b00_10 != 0 { Some(PixelColor::from(fc.0)) } else { None },
            stroke_color: if s & 0b10_00 != 0 { Some(PixelColor::from(sc.0)) } else { None },
            stroke_width: (s >> 16) as isize,
        }
    }
}

impl From<DrawStyle> for usize {
    fn from(draw_style: DrawStyle) -> usize {
        let sc: usize;
        if draw_style.stroke_color.is_some() {
            if draw_style.stroke_color.unwrap() == PixelColor::Dark {
                sc = 0b11;
            } else {
                sc = 0b10;
            }
        } else {
            sc = 0;
        }
        let fc: usize;
        if draw_style.fill_color.is_some() {
            if draw_style.fill_color.unwrap() == PixelColor::Dark {
                fc = 0b11;
            } else {
                fc = 0b10;
            }
            // fc = 0b10 | self.fill_color.unwrap() as usize; // this isn't working for some reason
        } else {
            fc = 0;
        }
        (draw_style.stroke_width as usize) << 16 | sc << 2 | fc
    }
}

#[cfg_attr(feature = "derive-rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelColor {
    Dark,
    Light,
}

impl From<bool> for PixelColor {
    fn from(pc: bool) -> Self { if pc { PixelColor::Dark } else { PixelColor::Light } }
}

impl From<PixelColor> for bool {
    fn from(pc: PixelColor) -> bool { if pc == PixelColor::Dark { true } else { false } }
}

impl From<usize> for PixelColor {
    fn from(pc: usize) -> Self { if pc == 0 { PixelColor::Light } else { PixelColor::Dark } }
}

impl From<PixelColor> for usize {
    fn from(pc: PixelColor) -> usize { if pc == PixelColor::Light { 0 } else { 1 } }
}

/// A single pixel
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pixel(pub Point, pub ColorNative);
