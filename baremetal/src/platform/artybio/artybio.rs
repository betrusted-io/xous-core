#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_MAIN_RAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_MAIN_RAM_MEM;

#[cfg(all(feature = "cramium-soc", not(feature = "verilator-only")))]
pub fn early_init() {}
