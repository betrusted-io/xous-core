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
// E000_0000 - E100_0000 => 16 MiB of vaddr space for page tables; should be more than enough
pub const SWAP_CFG_VADDR: usize = 0xE100_0000;
pub const SWAP_RPT_VADDR: usize = 0xE100_1000;
pub const SWAP_APP_UART_VADDR: usize = 0xE180_0000;
#[cfg(feature = "cramium-soc")]
pub const SWAP_APP_UART_IFRAM_VADDR: usize = 0xE180_1000;
// open a large aperture from A000-E000 for a potential RAM-mapped swap area: this gives us up to 1GiB swap
// space. Please don't actually use all of it: performance will be unimaginably bad.
pub const SWAP_HAL_VADDR: usize = 0xA000_0000;

/// Structure passed by the loader into this process at SWAP_RPT_VADDR
#[cfg(feature = "swap")]
#[repr(C)]
pub struct SwapSpec {
    pub key: [u8; 32],
    /// Count of PIDs in the system. Could be a u8, but, make it a u32 because we have
    /// the space and word alignment is good for stuff being tossed through unsafe pointers.
    pub pid_count: u32,
    pub rpt_len_bytes: u32,
    /// Base address of swap memory. If swap is memory-mapped, this is a virtual address.
    /// If swap is device-mapped, it's the physical offset in the device.
    pub swap_base: u32,
    /// Length of swap region in bytes
    pub swap_len: u32,
    /// Base of the message authentication code (MAC) region
    pub mac_base: u32,
    /// Length of the MAC region in bytes
    pub mac_len: u32,
}

// Locate the hard-wired IFRAM allocations for UDMA
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;
// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const APP_UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 3 * 4096;
// Flash will be released after the loader is done: it's only accessed to copy the IniS sectors into swap,
// then abandoned. It needs 4096 bytes for Rx, and 0 bytes for Tx + 16 bytes for cmd.
#[allow(dead_code)]
#[cfg(feature = "cramium-soc")]
pub const SPIM_FLASH_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 5 * 4096;
