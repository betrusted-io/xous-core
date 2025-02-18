use ux_api::minigfx::{ColorNative, FrameBuffer, Point};

pub const COLUMN: isize = 128;
pub const ROW: isize = 128;
pub const PAGE: u8 = ROW as u8 / 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonoColor(ColorNative);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mono {
    Black,
    White,
}
impl From<ColorNative> for Mono {
    fn from(value: ColorNative) -> Self {
        match value.0 {
            0 => Mono::Black,
            _ => Mono::White,
        }
    }
}
impl Into<ColorNative> for Mono {
    fn into(self) -> ColorNative {
        match self {
            Mono::Black => ColorNative::from(0),
            Mono::White => ColorNative::from(1),
        }
    }
}
