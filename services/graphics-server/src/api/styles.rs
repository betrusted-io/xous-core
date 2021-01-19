use crate::api::Point;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelColor {
    Dark,
    Light,
}

impl From<usize> for PixelColor {
    fn from(pc: usize) -> Self {
        if pc != 0 {
            PixelColor::Dark
        } else {
            PixelColor::Light
        }
    }
}

impl From<bool> for PixelColor {
    fn from(pc: bool) -> Self {
        if pc {
            PixelColor::Dark
        } else {
            PixelColor::Light
        }
    }
}

impl Into<usize> for PixelColor {
    fn into(self) -> usize {
        if self == PixelColor::Dark {
            1
        } else {
            0
        }
    }
}

impl Into<bool> for PixelColor {
    fn into(self) -> bool {
        if self == PixelColor::Dark {
            true
        } else {
            false
        }
    }
}

/// Style properties for an object
#[derive(Debug, Copy, Clone)]
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
        Self {
            fill_color: Some(fill),
            stroke_color: Some(stroke),
            stroke_width: width,
        }
    }

    /// Create a new style with a given stroke value and defaults for everything else
    pub fn stroke_color(stroke_color: PixelColor) -> Self {
        Self {
            stroke_color: Some(stroke_color),
            ..DrawStyle::default()
        }
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self {
            fill_color: Some(PixelColor::Dark),
            stroke_color: Some(PixelColor::Dark),
            stroke_width: 1,
        }
    }
}

impl From<usize> for DrawStyle {
    fn from(s: usize) -> Self {
        // usize split into these words:
        //  31 ...  16  15 ... 4     3..2    1..0
        //    width       rsvd      stroke   fill
        // where the MSB of stroke/fill encodes Some/None
        let fc: PixelColor = (s & 0b0001).into();
        let sc: PixelColor = (s & 0b0100).into();
        DrawStyle {
            fill_color: if s & 0b0010 != 0 { Some(fc) } else { None },
            stroke_color: if s & 0b1000 != 0 { Some(sc) } else { None },
            stroke_width: (s >> 16) as i16,
        }
    }
}

impl Into<usize> for DrawStyle {
    fn into(self) -> usize {
        let sc: usize;
        if self.stroke_color.is_some() {
            sc = 0b10 | self.stroke_color.unwrap() as usize;
        } else {
            sc = 0;
        }
        let fc: usize;
        if self.fill_color.is_some() {
            fc = 0b10 | self.fill_color.unwrap() as usize;
        } else {
            fc = 0;
        }
        (self.stroke_width as usize) << 16 | sc << 2 | fc
    }
}

/// A single pixel
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Pixel(pub Point, pub PixelColor);
