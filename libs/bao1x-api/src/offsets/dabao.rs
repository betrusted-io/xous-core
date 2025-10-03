// no external SPI
pub const SPI_FLASH_LEN: usize = 0;
pub const SPI_FLASH_ID_MASK: u32 = 0xff_ff_ff;
pub const SPI_FLASH_IDS: [u32; 0] = [];

// no external RAM
pub const SWAP_RAM_LEN: usize = 0;
pub const SWAP_RAM_ID_MASK: u32 = 0xff_ff;
// KGD 5D, mfg ID 9D; remainder of bits are part of the EID
pub const SWAP_RAM_IDS: [u32; 0] = [];

// "Partition table" of external SPI FLASH
pub const SWAP_FLASH_ORIGIN: usize = 0x0000_0000;
pub const SWAP_FLASH_RESERVED_LEN: usize = 0;
pub const APP_FLASH_ORIGIN: usize = 0;
pub const APP_FLASH_RESERVED_LEN: usize = 0;

// No PDDB, because no FLASH
pub const PDDB_ORIGIN: usize = 0;
pub const PDDB_LEN: usize = 0;

// Location of on-chip application segment, as offset from RRAM start
pub const APP_RRAM_OFFSET: usize = 0x30_0000;
pub const APP_RRAM_LEN: usize = 0xD_A000;
