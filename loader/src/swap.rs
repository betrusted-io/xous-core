/// Virtual address fields:
///  31            22 21               12 11               0
/// |    L1 index    |      L2 index     |    LSB of addr   |
///
/// L1 PTE thus consists of 1024 entries, each resolving to a 22-bit number.
///    - The bottom 10 bits are flags
///    - The top 2 bits are 0
///    - The middle 20 bits are the MSB of the address to the PA of the L2 PTE
///
/// L2 PTE thus consists of 1024 entries, each resolving to a 22-bit number. It is
/// indexed by the "L2 index" bits.
///    - The bottom 10 bits are flags
///    - The top 2 bits are 0
///    - The middle 20 bits are the MSB of the address to the PA of the target page

#[repr(C)]
pub struct SwapDescriptor {
    pub ram_offset: u32,
    pub ram_size: u32,
    pub name: u32,
    pub key: [u8; 32],
    pub flash_offset: u32,
}

#[derive(Debug)]
#[repr(C)]
pub struct SwapSourceHeader {
    pub version: u32,
    pub parital_nonce: [u8; 8],
    pub mac_offset: u32,
    pub aad_len: u32,
    // aad is limited to 64 bytes!
    pub aad: [u8; 64],
}

#[repr(C)]
pub struct RawPage {
    pub data: [u8; 4096],
}

pub const SWAP_PT_VADDR: usize = 0xE000_0000;
