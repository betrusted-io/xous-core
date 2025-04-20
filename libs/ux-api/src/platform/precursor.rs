pub const LINES: isize = 536;
pub const WIDTH: isize = 336;
pub const HEIGHT: isize = LINES;

pub const FB_WIDTH_WORDS: usize = 11;
pub const FB_WIDTH_PIXELS: usize = WIDTH as usize;
pub const FB_LINES: usize = LINES as usize;
pub const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines

// For passing frame buffer references
pub type FbRaw = [u32];
