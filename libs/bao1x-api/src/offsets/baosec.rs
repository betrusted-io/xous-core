use xous::arch::PAGE_SIZE;

use crate::offsets::{PartitionAccess, SlotIndex};

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

// Regulator voltage target at boot
pub const CPU_VDD_LDO_BOOT_MV: u32 = 810;
pub const DEFAULT_FCLK_FREQUENCY: u32 = 700_000_000;

// =========== KEY SLOTS ==============
/// The `ROOT_SEED` is the "mothership" secret that all device identity and secrets
/// are derived from. The seed may be blended with other bits of data scattered about
/// the RRAM array, other device identifiers and hardware measurements to create the final
/// device secret key.
pub const ROOT_SEED: SlotIndex = SlotIndex::Key(256, PartitionAccess::Fw0);

/// The `RMA_KEY` is a secret parameter that is unique per-device secret which is recorded
/// at manufacturing time. Its purpose is to facilitate the creation of a signed RMA
/// authorization certificate which upon receipt would blank the device and unlock various
/// features for debugging failed hardware.
pub const RMA_KEY: SlotIndex = SlotIndex::Key(257, PartitionAccess::Fw0);

/// Reserved for use as a CP to FT tracking cookie. This is used to help track inventory
/// between CP and FT, if such a feature is desired in the supply chain.
pub const CP_COOKIE: SlotIndex = SlotIndex::Key(258, PartitionAccess::Fw0);

/// `NUISANCE_KEYS` are hashed together with `ROOT_SEED` to derive the core secret.
/// Their primary purpose is to annoy microscopists trying to read the secret key by
/// directly imaging the RRAM array. They also exist to reduce power side channels
/// created upon accessing keys in combination with `CHAFF_KEYS`. Note there is ECC
/// on the data (2C2D on top of 128 bits) so any readout with better than 97% accuracy
/// can trivially rely on ECC to repair the results.
///
/// The placement is picked based upon two competing theories:
///   - Data stored next to each other will be harder to read out because the simultaneous activation of
///     read-out word lines via secondary electron leakage will cause the data in adjacent cells to
///     super-impose on the bitline.
///   - Data stored far from each other will be harder to read out based on the difficulty of achieving
///     flatness & uniformity in delayering over long distances.
///
/// Thus 2x 4k pages on either end of the key range are carved out for the nuisance keys.
pub const NUISANCE_KEYS_0: SlotIndex = SlotIndex::KeyRange(0..128, PartitionAccess::Fw0);
pub const NUISANCE_KEYS_1: SlotIndex = SlotIndex::KeyRange(1920..2048, PartitionAccess::Fw0);
pub const NUISANCE_KEYS: [SlotIndex; 2] = [NUISANCE_KEYS_0, NUISANCE_KEYS_1];

/// `CHAFF_KEYS` are a bank of keys that are hashed into the key array, but instead of
/// being read out in strict order, they are read in a random permutation every time.
/// The read-out data is XOR'd together into a single 256-bit key, and then hashed into
/// the key. The reason for this procedure is if there are strong power side channels in
/// the key read-out, the CHAFF_KEYS have a random ordering that frustrates attempts to
/// correlate the power signature over repeated reboots. The read-out of the CHAFF_KEYS
/// needs to be strictly constant time, i.e. the permutation function can't leak a
/// side channel that reveals what the ordering is.
pub const CHAFF_KEYS: SlotIndex = SlotIndex::KeyRange(128..256, PartitionAccess::Fw0);

/// All the slots of concern located in a single iterator. The idea is that everything is
/// condensed here and used to check for access integrity using the array below.
pub const ALL_SLOTS: [SlotIndex; 9] = [
    crate::offsets::SERIAL_NUMBER,
    crate::offsets::UUID,
    crate::offsets::IFR_HASH,
    ROOT_SEED,
    RMA_KEY,
    CP_COOKIE,
    NUISANCE_KEYS_0,
    NUISANCE_KEYS_1,
    CHAFF_KEYS,
];
