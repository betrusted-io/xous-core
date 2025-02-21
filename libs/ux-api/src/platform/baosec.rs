pub const LINES: isize = 128;
pub const HEIGHT: usize = LINES as usize;
pub const WIDTH: isize = 128;
pub const FB_SIZE: usize = LINES as usize * WIDTH as usize / core::mem::size_of::<u32>();
pub const FB_WIDTH_WORDS: usize = WIDTH as usize / core::mem::size_of::<u32>();
