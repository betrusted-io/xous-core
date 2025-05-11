use crate::{Error, MemoryAddress, MemoryFlags, MemoryRange};

// pub const DEFAULT_STACK_TOP: usize = 0x8000_0000;
pub const DEFAULT_HEAP_BASE: usize = 0x2000_0000;
pub const DEFAULT_MESSAGE_BASE: usize = 0x4000_0000;
pub const DEFAULT_BASE: usize = 0x6000_0000;

// open a large aperture from A000-E000 for a potential RAM-mapped swap area: this gives us up to 1GiB swap
// space. Please don't actually use all of it: performance will be unimaginably bad. Note that the
// A000-E000 range is also shared with the MMAP virtual region.
pub const SWAP_HAL_VADDR: usize = 0xa000_0000;
pub const MMAP_VIRT_BASE: usize = 0xb000_0000;

pub const USER_AREA_END: usize = 0xff00_0000;
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const THREAD_CONTEXT_AREA: usize = 0xff80_1000;

pub const FLG_VALID: usize = 0x1;
pub const FLG_R: usize = 0x2;
pub const FLG_W: usize = 0x4;
pub const FLG_X: usize = 0x8;
pub const FLG_U: usize = 0x10; // User
pub const FLG_A: usize = 0x40;
pub const FLG_D: usize = 0x80; // Dirty (explicitly managed, not automatic)
pub const FLG_S: usize = 0x100; // Shared
pub const FLG_P: usize = 0x200; // swaP

/// swap-specific flags
pub const SWAP_FLG_WIRED: u32 = 0x1_00;
pub const SWAP_PT_VADDR: usize = 0xE000_0000;
// E000_0000 - E100_0000 => 16 MiB of vaddr space for page tables; should be more than enough
pub const SWAP_CFG_VADDR: usize = 0xE100_0000;
pub const SWAP_RPT_VADDR: usize = 0xE100_1000;
pub const SWAP_COUNT_VADDR: usize = 0xE110_0000;
pub const SWAP_APP_UART_VADDR: usize = 0xE180_0000;
pub const SWAP_APP_UART_IFRAM_VADDR: usize = 0xE180_1000;

pub fn map_memory_pre(
    _phys: &Option<MemoryAddress>,
    _virt: &Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
) -> core::result::Result<(), Error> {
    Ok(())
}

pub fn map_memory_post(
    _phys: Option<MemoryAddress>,
    _virt: Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
    range: MemoryRange,
) -> core::result::Result<MemoryRange, Error> {
    Ok(range)
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), Error> { Ok(()) }

pub fn unmap_memory_post(_range: MemoryRange) -> core::result::Result<(), Error> { Ok(()) }
