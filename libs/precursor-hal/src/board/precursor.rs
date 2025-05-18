pub const FLASH_PHYS_BASE: u32 = 0x2000_0000;
pub const SOC_REGION_LOC: u32 = 0x0000_0000;
pub const SOC_REGION_LEN: u32 = 0x00D0_0000; // gw + staging + loader + kernel

// note to self: if these locations change, be sure to update the "filters" addresses
// in the gateware, so that we are consistent on what parts of the SPINOR are allowed access via USB debug
pub const SOC_MAIN_GW_LOC: u32 = 0x0000_0000; // gateware - primary loading address
pub const SOC_MAIN_GW_LEN: u32 = 0x0028_0000;
pub const SOC_STAGING_GW_LOC: u32 = 0x0028_0000; // gateware - staging copy
pub const SOC_STAGING_GW_LEN: u32 = 0x0028_0000;

pub const LOADER_LOC: u32 = 0x0050_0000; // loader - base
pub const LOADER_CODE_LEN: u32 = 0x0003_0000; // code region only
pub const LOADER_FONT_LOC: u32 = 0x0053_0000; // should be the same as graphics-server/src/fontmap.rs/FONT_BASE
pub const LOADER_FONT_LEN: u32 = 0x0044_0000; // length of font region only
pub const LOADER_TOTAL_LEN: u32 = LOADER_CODE_LEN + LOADER_FONT_LEN; // code + font

pub const EARLY_SETTINGS: u32 = 0x0097_0000;

pub const KERNEL_LOC: u32 = 0x0098_0000; // kernel start
pub const KERNEL_LEN: u32 = 0x0140_0000; // max kernel length = 0xA0_0000 * 2 => half the area for backup kernel & updates
pub const KERNEL_BACKUP_OFFSET: u32 = KERNEL_LEN - 0x1000; // last page of kernel is where the backup block gets located = 0x1D7_F000

pub const EC_REGION_LOC: u32 = 0x07F8_0000; // EC update staging area. Must be aligned to a 64k-address.
pub const EC_WF200_PKG_LOC: u32 = 0x07F8_0000;
pub const EC_WF200_PKG_LEN: u32 = 0x0004_E000;
pub const EC_FW_PKG_LOC: u32 = 0x07FC_E000;
pub const EC_FW_PKG_LEN: u32 = 0x0003_2000;
pub const EC_REGION_LEN: u32 = 0x0008_0000;

pub const PDDB_LOC: u32 = 0x01D8_0000; // PDDB start
pub const PDDB_LEN: u32 = EC_REGION_LOC - PDDB_LOC; // must be 64k-aligned (bulk erase block size) for proper function.

pub const SPINOR_ERASE_SIZE: u32 = 0x1000;

// quantum alloted to each process before a context switch is forced
// This is also platform-specific because it can be tuned based on CPU speed.
pub const BASE_QUANTA_MS: u32 = 10;

// sentinel used by test infrastructure to assist with parsing
// The format of any test infrastructure output to recover is as follows:
// _|TT|_<ident>,<data separated by commas>,_|TE|_
// where _|TT|_ and _|TE|_ are bookends around the data to be reported
// <ident> is a single-word identifier that routes the data to a given parser
// <data> is free-form data, which will be split at comma boundaries by the parser
pub const BOOKEND_START: &str = "_|TT|_";
pub const BOOKEND_END: &str = "_|TE|_";
