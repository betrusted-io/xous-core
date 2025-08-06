use utralib::{Field, Register};

pub const RGB_NUMREGS: usize = 1;
pub const HW_RGB_BASE: usize = 0xf0000000;
pub const OUT: Register = Register::new(0, 0xfff);
pub const OUT_OUT: Field = Field::new(12, 0, OUT);
pub const LD0: Field = Field::new(3, 0, OUT);
pub const LD1: Field = Field::new(3, 3, OUT);
pub const LD2: Field = Field::new(3, 6, OUT);
pub const LD3: Field = Field::new(3, 9, OUT);
