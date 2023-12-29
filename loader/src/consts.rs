pub const BACKUP_ARGS_ADDR: usize = crate::platform::RAM_BASE + crate::platform::RAM_SIZE - 0x2000;

pub const USER_STACK_TOP: usize = 0x8000_0000;
pub const PAGE_TABLE_OFFSET: usize = 0xff40_0000;
pub const PAGE_TABLE_ROOT_OFFSET: usize = 0xff80_0000;
pub const CONTEXT_OFFSET: usize = 0xff80_1000;
pub const USER_AREA_END: usize = 0xff00_0000;

// All of the kernel structures must live within Megapage 1023,
// and therefore are limited to 4 MB.
pub const EXCEPTION_STACK_TOP: usize = 0xffff_0000;
pub const KERNEL_STACK_TOP: usize = 0xfff8_0000;
pub const KERNEL_LOAD_OFFSET: usize = 0xffd0_0000;
pub const KERNEL_STACK_PAGE_COUNT: usize = 1;
pub const KERNEL_ARGUMENT_OFFSET: usize = 0xffc0_0000;
pub const GUARD_MEMORY_BYTES: usize = 3 * crate::PAGE_SIZE;
