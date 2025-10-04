use xous::arch::PAGE_SIZE;

// we actually have 16MiB on the initial prototypes, but constraining to smaller for cost reduction
pub const SPI_FLASH_LEN: usize = 8192 * 1024;
pub const SPI_FLASH_ID_MASK: u32 = 0xff_ff_ff;
// density 18, memory type 20, mfg ID C2 ==> MX25L128833F
// density 38, memory type 25, mfg ID C2 ==> MX25U12832F
// mfg ID 0b ==> XT25Q64FWOIGT cost down option (8MiB)
pub const SPI_FLASH_IDS: [u32; 3] = [0x1820c2, 0x3825c2, 0x17_60_0b];

// details of the SPINOR device
pub const SPINOR_PAGE_LEN: u32 = 0x100;
pub const SPINOR_ERASE_SIZE: u32 = 0x1000; // this is the smallest sector size.
pub const SPINOR_BULK_ERASE_SIZE: u32 = 0x1_0000; // this is the bulk erase size.

// 4MiB vs 8MiB memory price is a very nominal difference (~$0.20 off of $2.40 budgetary)
// So it's either all-or-nothing: either we have 8MiB, or we have none, in the final
// configuration.
pub const SWAP_RAM_LEN: usize = 8192 * 1024;
pub const SWAP_RAM_ID_MASK: u32 = 0xff_ff;
// KGD 5D, mfg ID 9D; remainder of bits are part of the EID
pub const SWAP_RAM_IDS: [u32; 2] = [0x5d9d, 0x559d];

// Location of things in external SPIM flash
pub const SWAP_FLASH_ORIGIN: usize = 0x0000_0000;
// maximum length of a swap image
pub const SWAP_FLASH_RESERVED_LEN: usize = 4096 * 1024;
// Total area reserved for the swap header, including the signature block.
pub const SWAP_HEADER_LEN: usize = PAGE_SIZE;

// PDDB takes up "the rest of the space" - about 4MiB envisioned. Should be
// "enough" for storing a few hundred, passwords, dozens of x.509 certs, dozens of private keys, etc.
pub const PDDB_ORIGIN: usize = SWAP_FLASH_ORIGIN + SWAP_FLASH_RESERVED_LEN;
pub const PDDB_LEN: usize = SPI_FLASH_LEN - PDDB_ORIGIN;

// Location of on-chip application segment, as offset from RRAM start
pub const APP_RRAM_OFFSET: usize = 0;
pub const APP_RRAM_LEN: usize = 0;
