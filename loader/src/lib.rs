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

pub const SWAP_FLG_WIRED: u32 = 0x1_00;
// Note: this flag is currently not used. It was an aborted attempt to do
// auto-write on dirty page but due to lack of hw synchronization we couldn't
// guarantee atomicity. Flag can be removed or re-used in the future if another
// attempt is tried on a system that has hardware dirty page updating.
pub const SWAP_FLG_DIRTY: u32 = 0x2_00;
pub const FLG_SWAP_USED: u32 = 0x8000_0000;

// Locate the hard-wired IFRAM allocations for UDMA
#[allow(dead_code)]
#[cfg(feature = "bao1x")]
pub const UART_IFRAM_ADDR: usize = bao1x_hal::board::UART_DMA_TX_BUF_PHYS;
#[allow(dead_code)]
#[cfg(feature = "bao1x")]
pub const APP_UART_IFRAM_ADDR: usize = bao1x_hal::board::APP_UART_IFRAM_ADDR;

/// This is the amount of space that the loader stack will occupy as it runs, assuming no swap and giving one
/// page for the clean suspend marker
#[cfg(not(feature = "swap"))]
pub const GUARD_MEMORY_BYTES: usize = 3 * crate::PAGE_SIZE;
/// Amount of space for loader stack only, with swap
#[cfg(all(feature = "swap", not(feature = "resume"), not(feature = "bao1x")))]
pub const GUARD_MEMORY_BYTES: usize = 7 * crate::PAGE_SIZE;
#[cfg(all(feature = "swap", not(feature = "resume"), feature = "bao1x"))]
pub const GUARD_MEMORY_BYTES: usize = 7 * crate::PAGE_SIZE;
/// Amount of space for loader stack plus clean suspend, with swap
#[cfg(all(feature = "swap", feature = "resume"))]
pub const GUARD_MEMORY_BYTES: usize = 8 * crate::PAGE_SIZE; // 1 extra page for clean suspend

#[cfg(feature = "swap")]
pub const SWAPPER_PID: u8 = 2;

#[cfg(feature = "bao1x")]
pub const SYSTEM_CLOCK_FREQUENCY: u32 = 800_000_000;
