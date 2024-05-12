#![no_std]

#[cfg(feature = "swap")]
pub mod swap;

pub const PAGE_SIZE: usize = 4096;
pub type XousPid = u8;
#[cfg(not(feature = "swap"))]
pub type XousAlloc = XousPid;
#[cfg(feature = "swap")]
pub type XousAlloc = swap::SwapAlloc;

pub const FLG_VALID: usize = 0x1;
pub const FLG_X: usize = 0x8;
pub const FLG_W: usize = 0x4;
pub const FLG_R: usize = 0x2;
pub const FLG_U: usize = 0x10;
#[cfg(not(feature = "atsama5d27"))]
pub const FLG_A: usize = 0x40;
#[cfg(not(feature = "atsama5d27"))]
pub const FLG_D: usize = 0x80;
pub const FLG_P: usize = 0x200;

pub const FLG_SWAP_USED: u32 = 0x8000_0000;

// Locate the hard-wired IFRAM allocations for UDMA
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;
