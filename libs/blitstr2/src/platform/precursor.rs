pub const LINES: isize = 536;
pub const WIDTH: isize = 336;
pub const WORDS_PER_LINE: usize = 11;
pub type FrBuf = [u32; WORDS_PER_LINE * LINES as usize];
pub const FB_SIZE: usize = WORDS_PER_LINE * LINES as usize; // 44 bytes by 536 lines
