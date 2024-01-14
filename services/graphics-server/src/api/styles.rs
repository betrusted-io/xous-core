use crate::api::Point;

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

/// Style properties for an object
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct DrawStyle {
    /// Fill colour of the object
    pub fill_color: Option<PixelColor>,

    /// Stroke (border/line) color of the object
    pub stroke_color: Option<PixelColor>,

    /// Stroke width
    pub stroke_width: i16,
}

impl DrawStyle {
    pub fn new(fill: PixelColor, stroke: PixelColor, width: i16) -> Self {
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
        let fc: PixelColor = (s & 0b00_01).into();
        let sc: PixelColor = (s & 0b01_00).into();
        DrawStyle {
            fill_color: if s & 0b00_10 != 0 { Some(fc) } else { None },
            stroke_color: if s & 0b10_00 != 0 { Some(sc) } else { None },
            stroke_width: (s >> 16) as i16,
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

/// A single pixel
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pixel(pub Point, pub PixelColor);
