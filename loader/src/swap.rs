use crate::{bootconfig::BootConfig, PageTable};

#[repr(C)]
pub struct SwapDescriptor {
    pub ram_offset: u32,
    pub ram_size: u32,
    pub name: u32,
    pub key: [u8; 32],
    pub flash_offset: u32,
}

#[repr(C)]
pub struct SwapSourceHeader {
    pub version: u32,
    pub parital_nonce: [u8; 8],
    pub mac_offset: u32,
    pub aad_len: u32,
    // consumes up to the remainder of the page
    pub aad: [u8; 4076],
}

/// An aligned, raw-page structure
#[repr(C, align(4096))]
pub struct RawPage {
    pub data: [u8; 4096],
}

/// Mapping pages to swap.
///
/// Given a virtual address and PID, map to a physical offset in swap.
/*
bitfield! {
    #[derive(Copy, Clone, Eq)]
    impl Debug;
    pub u32,
}
pub struct SwapPte {}
*/

const FLG_VALID: usize = 0x1;

/// Virtual address fields:
///  31            22 21               12 11               0
/// |    L1 index    |      L2 index     |    LSB of addr   |
///
/// L1 PTE thus consists of 1024 entries, each resolving to a 22-bit number.
///    - The bottom 10 bits are flags
///    - The top 2 bits are 0
///    - The middle 20 bits are the MSB of the address to the PA of the L2 PTE
pub fn set_l1_pte(from_va: usize, to_pa: usize, root_pt: &mut PageTable) {
    let index = from_va >> 22;
    root_pt.entries[index] = ((to_pa & 0xFFFF_FC00) >> 2) // top 2 bits of PA are not used, we don't do 34-bit PA featured by Sv32
        | FLG_VALID;
}

/// Virtual address fields:
///  31            22 21               12 11               0
/// |    L1 index    |      L2 index     |    LSB of addr   |
///
/// L2 PTE thus consists of 1024 entries, each resolving to a 22-bit number. It is
/// indexed by the "L2 index" bits.
///    - The bottom 10 bits are flags
///    - The top 2 bits are 0
///    - The middle 20 bits are the MSB of the address to the PA of the target page
pub fn set_l2_pte(from_va: usize, to_pa: usize, l2_pt: &mut PageTable, flags: usize) {
    let index = (from_va >> 12) & 0x3_FF;
    l2_pt.entries[index] = ((to_pa & 0xFFFF_FC00) >> 2) // top 2 bits of PA are not used, we don't do 34-bit PA featured by Sv32
        | flags
        | FLG_VALID;
}
