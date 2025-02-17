pub const LINES: isize = 128;
pub const WIDTH: isize = 128;
pub const WORDS_PER_LINE: usize = WIDTH as usize / core::mem::size_of::<u32>();
pub type FrBuf = [u32; WORDS_PER_LINE * LINES as usize];
pub const FB_SIZE: usize = WORDS_PER_LINE * LINES as usize;
