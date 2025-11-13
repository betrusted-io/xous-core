use xous::arch::PAGE_SIZE;

use crate::offsets::{PartitionAccess, RwPerms, SlotIndex};

// we actually have 16MiB on the initial prototypes, but constraining to smaller for cost reduction
pub const SPI_FLASH_LEN: usize = 8192 * 1024;
pub const SPI_FLASH_ID_MASK: u32 = 0xff_ff_ff;

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
pub const ROOT_SEED: SlotIndex = SlotIndex::Key(256, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// The `RMA_KEY` is a secret parameter that is unique per-device secret which is recorded
/// at manufacturing time. Its purpose is to facilitate the creation of a signed RMA
/// authorization certificate which upon receipt would blank the device and unlock various
/// features for debugging failed hardware.
pub const RMA_KEY: SlotIndex = SlotIndex::Key(257, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// Reserved for use as a CP to FT tracking cookie. This is used to help track inventory
/// between CP and FT, if such a feature is desired in the supply chain. Blanked on entry
/// to developer mode.
pub const CP_COOKIE: SlotIndex = SlotIndex::Key(258, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// The swap encryption key. Used to protect swap images beyond the signing key, if we so desire.
pub const SWAP_KEY: SlotIndex = SlotIndex::Key(259, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// `NUISANCE_KEYS` are hashed together with `ROOT_SEED` to derive the core secret.
/// Their primary purpose is to annoy microscopists trying to read the secret key by
/// directly imaging the RRAM array. They also exist to reduce power side channels
/// created upon accessing keys in combination with `CHAFF_KEYS`. Note there is ECC
/// on the data (2C2D on top of 128 bits) so any readout with better than 97% accuracy
/// can trivially rely on ECC to repair the results (and now you have a metric for how
/// reliable your readout needs to be).
///
/// The placement is picked based upon two competing theories:
///   - Data stored next to each other will be harder to read out because the simultaneous activation of
///     read-out word line transistors via secondary electron leakage will cause the data in adjacent cells to
///     super-impose on the bitline. You want to have several random bits superimposed to defeat probabilistic
///     models of leakage behavior.
///   - Data stored far from each other will be harder to read out based on the difficulty of achieving
///     flatness & uniformity in delayering over long distances.
///
/// Thus 2x 4k pages on either end of the key range are carved out for the nuisance keys. 4k page size
/// is convenient because that's the natural page size over which the key range will be carved up before
/// handing to other processes.
///
/// The first 8 keys in bank 0 of NUISANCE_KEYS is not used. This is because due to an ECO in the A1
/// spin of silicon, the JTAG-wired access control on these is going to be tied to those on the similarly
/// numbered data slots. Some of the data slots in the first 8 slots will be read-only at CP time, and
/// thus they can't be initialized with random data and be used as a nuisance key. This is a minor degradation
/// in security margin.
pub const NUISANCE_KEYS_0: SlotIndex = SlotIndex::KeyRange(8..128, PartitionAccess::Fw0, RwPerms::ReadWrite);
pub const NUISANCE_KEYS_1: SlotIndex =
    SlotIndex::KeyRange(1920..2048, PartitionAccess::Fw0, RwPerms::ReadWrite);
pub const NUISANCE_KEYS: [SlotIndex; 2] = [NUISANCE_KEYS_0, NUISANCE_KEYS_1];

/// `CHAFF_KEYS` are a bank of keys that are hashed into the key array, but instead of
/// being read out in strict order, they are read in a random permutation every time.
/// The read-out data is XOR'd together into a single 256-bit key, and then hashed into
/// the key. The reason for this procedure is if there are strong power side channels in
/// the key read-out, the CHAFF_KEYS have a random ordering that frustrates attempts to
/// correlate the power signature over repeated reboots. The read-out of the CHAFF_KEYS
/// needs to be strictly constant time, i.e. the permutation function can't leak a
/// side channel that reveals what the ordering is. Better yet, CHAFF_KEYS can be read
/// out interleaved with the NUISANCE_KEY readout, where the selection to read a CHAFF_KEY
/// or a NUISANCE_KEY is done randomly, thus introducing some disorder in the power side
/// channel timing of the NUISANCE_KEY readout.
///
/// This block has ReadWrite permissions because it's what gets blanked when the system
/// goes to developer mode. We don't blank *all* the keys because write-permission on a key
/// can lead to "oracle" attacks where portions of the key can be guessed by setting individual
/// bits. But this is enough bits that it should make the original key unrecoverable. Once
/// the chaff is cleared, we also don't care as much about side channels since we're now operating
/// in a fundamentally insecure regime (e.g. developer mode - you can just read out the data by
/// running your own code on the device).
pub const CHAFF_KEYS: SlotIndex = SlotIndex::KeyRange(128..256, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// All the slots of concern located in a single iterator. The idea is that everything is
/// condensed here and used to check for access integrity using the array below.
pub const DATA_SLOTS: [SlotIndex; 8] = [
    crate::offsets::SERIAL_NUMBER,
    crate::offsets::UUID,
    crate::offsets::IFR_HASH,
    crate::offsets::CP_ID,
    crate::offsets::BAO1_PUBKEY,
    crate::offsets::BAO2_PUBKEY,
    crate::offsets::BETA_PUBKEY,
    crate::offsets::DEV_PUBKEY,
];

/// In addition to these KEY_SLOTS, the DEVELOPER_MODE one way counter is a security-important parameter
/// that should be included as domain separation in any KDF.
pub const KEY_SLOTS: [SlotIndex; 6] =
    [ROOT_SEED, RMA_KEY, CP_COOKIE, NUISANCE_KEYS_0, NUISANCE_KEYS_1, CHAFF_KEYS];
