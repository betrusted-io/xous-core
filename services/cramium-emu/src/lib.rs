pub mod camera;
pub mod display;
pub mod i2c;
pub mod keyboard;
pub mod trng;
pub mod udma;

pub const SPINOR_PAGE_LEN: u32 = 0x100;
pub const SPINOR_ERASE_SIZE: u32 = 0x1000; // this is the smallest sector size.
pub const SPINOR_BULK_ERASE_SIZE: u32 = 0x1_0000; // this is the bulk erase size.
pub const SPINOR_LEN: u32 = 16384 * 1024;
pub const PDDB_LOC: u32 = 4096 * 1024; // located 4MiB in, after the swap image
pub const PDDB_LEN: u32 = 4096 * 1024; // 4MiB data for the PDDB total
