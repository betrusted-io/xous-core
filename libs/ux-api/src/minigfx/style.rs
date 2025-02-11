use crate::minigfx::Point;

/// Type wrapper for native colors
#[derive(Clone, Copy, PartialEq, Eq, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ColorNative(pub usize);
impl From<usize> for ColorNative {
    fn from(value: usize) -> Self { Self { 0: value } }
}
impl Into<usize> for ColorNative {
    fn into(self) -> usize { self.0 }
}

/// Style properties for an object
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<ColorNative>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<ColorNative>,

    /// Stroke width
    pub stroke_width: isize,
}

impl DrawStyle {
    pub fn new(fill: ColorNative, stroke: ColorNative, width: isize) -> Self {
        Self { fill_color: Some(fill), stroke_color: Some(stroke), stroke_width: width }
    }

    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: ColorNative) -> Self {
        Self { stroke_color: Some(stroke_color), ..DrawStyle::default() }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self { fill_color: Some(ColorNative(0)), stroke_color: Some(ColorNative(0)), stroke_width: 1 }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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
