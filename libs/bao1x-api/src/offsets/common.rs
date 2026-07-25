use crate::offsets::PartitionAccess;
use crate::offsets::RwPerms;
use crate::offsets::SlotIndex;

// These define the start region of the partition. In general, each partition has signature
// metadata in them, but the first word in the signature is a jump instruction that takes you
// to the actual boot code.
pub const BOOT0_START: usize = 0x6000_0000;
pub const BOOT1_START: usize = 0x6002_0000;
pub const LOADER_START: usize = 0x6006_0000;
pub const BAREMETAL_START: usize = LOADER_START;
// kernel needs to start on a page boundary, so eat into the loader area a bit to allow that to happen.
pub const KERNEL_START: usize = 0x600A_0000 - crate::signatures::SIGBLOCK_LEN;

// total storage area available in RRAM. Above this are reserved vectors for security apparatus.
pub const RRAM_STORAGE_LEN: usize = 0x3D_A000;

// loadable swap "starts" at these address for UF2 updates. They are are interpreted as
// zero-offsets from their respective "partitions" after masking for the top address location.
pub const SWAP_START_UF2: usize = 0x7000_0000;
pub const SWAP_UF2_LEN: usize = 0x0800_0000; // 128 MiB

#[derive(Clone, Copy, Debug, PartialEq, Eq, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum OneWayErr {
    OutOfBounds,
    IncFail,
    InvalidCoding,
    // used to report messaging errors in xous environments
    InternalError,
    // used to indicate things are OK in xous environments
    None,
}

// Define a trait with just the offset
pub trait OneWayEncoding: TryFrom<u32> {
    /// First one-way counter slot backing this encoding.
    const OFFSET: usize;
    /// How many slots the encoding occupies. Always 1 for `encode_oneway!` enums, since the
    /// variant is the counter value modulo the variant count. Present so that a future
    /// multi-slot encoding registers its real footprint with the slot map.
    const SLOTS: usize = 1;
    /// Type name, used by the slot map diagnostics to report collisions by name.
    const NAME: &'static str;
    /// Registration entry for `OWC_MAP`. Derived from the three consts above, so it cannot
    /// disagree with the `#[offset = N]` attribute the way a hand-copied literal could.
    const CLAIM: OwcClaim = OwcClaim { name: Self::NAME, first: Self::OFFSET, count: Self::SLOTS };
}

macro_rules! encode_oneway {
    (
        #[offset = $offset:literal]
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $variant:ident
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u32)]
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum $name {
            $(
                $variant
            ),*
        }

        impl $name {
            const ALL: &'static [Self] = &[
                $(Self::$variant),*
            ];
        }
        // A variant list must be non-empty: `value % 0` in the decode below would panic at
        // runtime, and the macro pattern happily accepts `pub enum Foo {}`. Drop this if you
        // would rather not have the extra const item.
        const _: () = assert!(
            $name::ALL.len() != 0,
            concat!("encode_oneway!: `", stringify!($name), "` declares no variants")
        );
        impl TryFrom<u32> for $name {
            type Error = ();

            fn try_from(value: u32) -> Result<Self, Self::Error> {
                let index = (value % Self::ALL.len() as u32) as usize;
                Ok(Self::ALL[index])
            }
        }

        impl OneWayEncoding for $name {
            const OFFSET: usize = $offset;
            const NAME: &'static str = stringify!($name);
        }
    };
}

/// Total number of public key slots in the system. Pubkey revocations are duplicated because
/// being able to override a pubkey revocation is a strong attack vector. If either OWC reads back
/// as non-zero, the pubkey is considered to be revoked.
pub const PUBKEY_SLOTS: usize = 4;

pub const fn classic_to_pq_revocation(classic: usize) -> Option<usize> {
    match classic {
        BOOT0_REVOCATION_OFFSET => Some(PQ_BOOT0_REVOCATION_OFFSET),
        BOOT1_REVOCATION_OFFSET => Some(PQ_BOOT1_REVOCATION_OFFSET),
        LOADER_REVOCATION_OFFSET => Some(PQ_LOADER_REVOCATION_OFFSET),
        _ => None,
    }
}

// =========== ONE WAY COUNTER SLOTS ==============
// slots 0-127 are for bootloader/xous use
// slots 128-255 are for user applications

// slots 0-17 are currently unallocated

encode_oneway! {
    #[offset = 18]
    pub enum UsbDefaultSpeed {
        High,
        Full,
    }
}

/// When this or REQUIRE_PQ_DUPE is set, the next stage *must* have a valid PQ signature
/// By default, PQ signatures are optional.
pub const REQUIRE_PQ: usize = 19;

pub const PQ_LOADER_REVOCATION_DUPE_OFFSET: usize = 20; // [20..=23]
pub const PQ_BOOT1_REVOCATION_DUPE_OFFSET: usize = PQ_LOADER_REVOCATION_DUPE_OFFSET + PUBKEY_SLOTS; // [24..=27]
pub const PQ_BOOT0_REVOCATION_DUPE_OFFSET: usize = PQ_BOOT1_REVOCATION_DUPE_OFFSET + PUBKEY_SLOTS; // [28..=31]

/// Offset in the one-way counter array for PQ loader key revocations. Provisions for up to four
/// key slots, from [32..=35].
pub const PQ_LOADER_REVOCATION_OFFSET: usize = 32;
/// Offset in the one-way counter array for boot1 key revocations. Provisions for up to four
/// key slots, from [36..=39].
pub const PQ_BOOT1_REVOCATION_OFFSET: usize = PQ_LOADER_REVOCATION_OFFSET + PUBKEY_SLOTS;
/// Offset in the one-way counter array for boot0 key revocations. Provisions for up to four
/// key slots, from [40..=43].
pub const PQ_BOOT0_REVOCATION_OFFSET: usize = PQ_BOOT1_REVOCATION_OFFSET + PUBKEY_SLOTS;

/// Fixed offset between the main key and the duplicate key
pub const PQ_REVOCATION_DUPE_DISTANCE: usize = PQ_LOADER_REVOCATION_OFFSET - PQ_LOADER_REVOCATION_DUPE_OFFSET;

/// See REQUIRE_PQ. Duplicate value to complicate glitching/editing attacks.
pub const REQUIRE_PQ_DUPE: usize = 44;

// slots 45, 46 are unallocated

/// This is used to count the number of boots, up to a certain threshold. The main
/// purpose is to turn off the "audit" auto-print which is used to verify chip testing.
pub const EARLY_BOOT_COUNT: usize = 47;

// gap of 8 slots [48..=55] left in case more anti-rollback slots are desired

/// Anti-rollback counters only count up and are used to note what is the minimum rollback
/// version allowed for a firmware stage. Note that this is different from a semver because
/// semvers can change without there being security problems. Anti-rollback counter is in
/// principle limited to 10k increments due to wear out of the underlying memory, so we
/// use a separate counter for that.
pub const RESERVED0_ANTI_ROLLBACK: usize = 56;
pub const RESERVED1_ANTI_ROLLBACK: usize = 57;
pub const BOOT0_ANTI_ROLLBACK: usize = 58;
pub const BOOT1_ANTI_ROLLBACK: usize = 59;
pub const LOADER_ANTI_ROLLBACK: usize = 60;
pub const KERNEL_ANTI_ROLLBACK: usize = 61;
pub const SWAP_ANTI_ROLLBACK: usize = 62;
pub const APP_ANTI_ROLLBACK: usize = 63;
pub const BAREMETAL_ANTI_ROLLBACK: usize = 64;

/// Paranoid mode is written twice; either being non-zero invokes paranoid mode
/// In paranoid mode, the glitch detectors are set to trigger aggressively and reset the
/// system through automatic hardware means. This can lead to false positives that can
/// degrade user experience, which is why it's left as a setting.
pub const PARANOID_MODE: usize = 65;

/// Counter that logs the possible number of attacks seen. This is used in conjunction
/// with paranoid mode to initiate a system wipe. The reason this can be non-zero is that
/// stray light or environmental factors can trigger the system, and so this threshold is
/// tunable.
pub const POSSIBLE_ATTACKS: usize = 66;

/// Paranoid mode is written twice
pub const PARANOID_MODE_DUPE: usize = 67;

/// The layout of the below duplicate revocation bits *MUST* match that of the primary keys.
/// This property is relied upon by the signature checking routine.
/// Offset in the one-way counter array for loader key revocations. Provisions for up to four
/// key slots, from [68..=71].
pub const LOADER_REVOCATION_DUPE_OFFSET: usize = 68;
/// Offset in the one-way counter array for boot1 key revocations. Provisions for up to four
/// key slots, from [72..=75].
pub const BOOT1_REVOCATION_DUPE_OFFSET: usize = LOADER_REVOCATION_DUPE_OFFSET + PUBKEY_SLOTS;
/// Offset in the one-way counter array for boot0 key revocations. Provisions for up to four
/// key slots, from [76..=79].
pub const BOOT0_REVOCATION_DUPE_OFFSET: usize = BOOT1_REVOCATION_DUPE_OFFSET + PUBKEY_SLOTS;

/// Fixed offset between the main key and the duplicate key
pub const REVOCATION_DUPE_DISTANCE: usize = LOADER_REVOCATION_OFFSET - LOADER_REVOCATION_DUPE_OFFSET;

encode_oneway! {
    #[offset = 80]
    pub enum BootWaitCoding {
        Disable,
        Enable,
    }
}

encode_oneway! {
    #[offset = 81]
    pub enum BoardTypeCoding {
        Dabao,
        Baosec,
        Oem,
    }
}

encode_oneway! {
    #[offset = 82]
    pub enum AltBootCoding {
        PrimaryPartition,
        AlternatePartition,
    }
}

/// Incremented from 0 if chip-probe boot setup has been finished.
pub const CP_BOOT_SETUP_DONE: usize = 83;

/// Incremented from 0 if system boot setup has been finished. This is for systems that have
/// a supplemental entropy source and want to replace the generated keys with keys that are
/// derived from a blend of entropy sources. It's only useful on platforms like baosec.
pub const IN_SYSTEM_BOOT_SETUP_DONE: usize = 84;

/// When non-zero, the system had, at least one point in time, been challenged to boot
/// from a developer image. Thus, the state of the system cannot be attested to based on
/// the original signing keys burned from the factory. The value of this is also
/// included as AAD in key derivations.
pub const DEVELOPER_MODE: usize = 85;

/// This is flipped when a trust transfer happens to a third party. i.e. any OEMs that
/// come to Baochip to sign an image (that may then have their public keys in it) are
/// required to set this bit as part of their signed code. It's a half-baked work-around
/// for folks that are paranoid about DEVELOPER_MODE and for whatever reason think it's
/// more trustworthy if someone they've never met used some cryptography to bless a bag
/// of bits, but at the least they can say that a person they don't know or trust most likely
/// did bless the bag of bits.
pub const OEM_MODE: usize = 86;

/// This is incremented if the boot0 public keys failed to compare against the static keys in
/// the data store.
pub const BOOT0_PUBKEY_FAIL: usize = 87;
/// This is incremented if the boot1 public keys failed to compare against the static keys in
/// the data store. Note that on Alpha-rev dabaos, this was used to track the initialization
/// of dabao security state, and so this may be set already due to that use.
pub const BOOT1_PUBKEY_FAIL: usize = 88;

encode_oneway! {
    #[offset = 90]
    /// When set, the system will prefer to present generic, fixed identifiers when challenged
    /// by external systems. The canonical use case for this is the serial number field in the
    /// USB device descriptor: normally, ExternalIdentifiers is `0`, which means the device will
    /// present a semi-unique serial number (this is useful for users who plug in multiple devices
    /// and want to tell them apart). However, privacy-conscious users who don't need or want
    /// to tell devices apart can increment this OWC and then the USB serial number will be
    /// replaced with a fixed pattern that is common across all devices.
    pub enum ExternalIdentifiers{
        SerialNumber,
        Anonymous,
    }
}

/// Offset in the one-way counter array for loader key revocations. Provisions for up to four
/// key slots, from [116..=119].
pub const LOADER_REVOCATION_OFFSET: usize = 116;
/// Offset in the one-way counter array for boot1 key revocations. Provisions for up to four
/// key slots, from [120..=123].
pub const BOOT1_REVOCATION_OFFSET: usize = LOADER_REVOCATION_OFFSET + PUBKEY_SLOTS;
/// Offset in the one-way counter array for boot0 key revocations. Provisions for up to four
/// key slots, from [124..=127].
pub const BOOT0_REVOCATION_OFFSET: usize = BOOT1_REVOCATION_OFFSET + PUBKEY_SLOTS;

// slots from 128..=255 are totally unused by the boot logic

// =========== DATA SLOTS ==============

/// The 'SERIAL_NUMBER` is a publicly readable number that has a "weak" guarantee of
/// uniqueness, in that there is nothing essentially that prevents duplicates, forgeries
/// or procedural errors replicating this. The serial number also is not strictly incrementing
/// nor does it have any guarantee of being a monotonic or smoothly spaced out. It could
/// even be all zeros (in which case LOT_CODE should be used). However, nominally, the plan
/// is for SERIAL_NUMBER to be exactly the CP_ID field.
pub const SERIAL_NUMBER: SlotIndex = SlotIndex::Data(0, PartitionAccess::Open, RwPerms::ReadWrite);

/// `UUID` is a 256-bit random number that can be used as a UUID for the chip. It is publicly
/// readable and generated by a TRNG. This is suitable for putting into a KDF and generating
/// salts for algorithms that require such a parameter. The UUID is changeable, but changing
/// will cause all derived keys to be changed as well.
pub const UUID: SlotIndex = SlotIndex::Data(1, PartitionAccess::Open, RwPerms::ReadWrite);

/// `IFR_HASH` is a provisional slot for a hash of the IFR region. Whether the hash is meaningful
/// or not depends on if the chip is booted before it is sealed. At the time of writing, it's
/// not clear if the wafer probe infrastructure will allow this.
pub const IFR_HASH: SlotIndex = SlotIndex::Data(2, PartitionAccess::Open, RwPerms::ReadWrite);

/// `WAFER_ID` is a copy of the lot ID + wafer ID + x/y position data that should be captured
/// during CP.
pub const CP_ID: SlotIndex = SlotIndex::Data(3, PartitionAccess::Open, RwPerms::ReadWrite);

/// Indelible versions of the public keys. The problem with the pubkeys in boot0 region is that
/// boot0 itself has the ability to modify its own memory. A copy here can have a bit set in
/// the IFR that blocks any attempt to modify these keys.
pub const BAO1_PUBKEY: SlotIndex = SlotIndex::Data(4, PartitionAccess::All, RwPerms::ReadOnly);
pub const BAO2_PUBKEY: SlotIndex = SlotIndex::Data(5, PartitionAccess::All, RwPerms::ReadOnly);
pub const BETA_PUBKEY: SlotIndex = SlotIndex::Data(6, PartitionAccess::All, RwPerms::ReadOnly);
pub const DEV_PUBKEY: SlotIndex = SlotIndex::Data(7, PartitionAccess::All, RwPerms::ReadOnly);

// [8..=260 used as nuisance and master keys]

/// Collateral is a data range that is *always* erased every time the system transitions from boot0
/// to a Baochip-signed boot1. The only condition under which it is not erased is if *all* the boot1 keys
/// in slots 0, 1, *and* 2 do *not* match the Baochip set.
///
/// The purpose of the collateral keys is to ensure that no Baochip-signed images can be used to
/// decrypt third-party signed images. Thus, the conditions for signing a third-party image with
/// its own signing keys are as follows:
///
/// - It must be a Boot1 image
/// - Keys 0, 1, and 2 must all be different from the Baochip keys
/// - The third-party firmware must generate and populate all the COLLATERAL data slots.
/// - The third-party firmware must incorporate at least one of the keys in slots 261, 262, or 263 into their
///   root key mechanism.
/// - Collateral key in slot 264 must be made disclosable through a public inspection mechanism. The purpose
///   of the inspection is to verify that in fact the collateral keys have been populated with non-zero value
///   by the third-party firmware, and to verify erase of the key range when necessary. Erasure always
///   progresses from low slot to high slot, and thus one can infer the erasure state of the collateral by
///   inspecting the value of the high key slot.
pub const COLLATERAL: SlotIndex = SlotIndex::DataRange(261..265, PartitionAccess::Fw0, RwPerms::ReadWrite);

/// Boot1 pubkey `receipt` fields record the last accepted public key used when running boot1.
/// If this changes, the collateral keys need to be erased. This prevents one third-party signed firmware
/// from being used to attack another third-party signed firmware. These keys have a 1:1 correlation
/// with the BAO1, BAO2, BETA, and DEV _PUBKEY slots; the data type doesn't use a range so that iterators
/// can have a cleaner parallel structure when comparing against these.
pub const BOOT1_PK_RECEIPT_SLOT0: SlotIndex = SlotIndex::Data(265, PartitionAccess::Open, RwPerms::ReadWrite);
pub const BOOT1_PK_RECEIPT_SLOT1: SlotIndex = SlotIndex::Data(266, PartitionAccess::Open, RwPerms::ReadWrite);
pub const BOOT1_PK_RECEIPT_SLOT2: SlotIndex = SlotIndex::Data(267, PartitionAccess::Open, RwPerms::ReadWrite);
pub const BOOT1_PK_RECEIPT_SLOT3: SlotIndex = SlotIndex::Data(268, PartitionAccess::Open, RwPerms::ReadWrite);
pub const BOOT1_RECEIPT_SLOTS: [SlotIndex; 4] =
    [BOOT1_PK_RECEIPT_SLOT0, BOOT1_PK_RECEIPT_SLOT1, BOOT1_PK_RECEIPT_SLOT2, BOOT1_PK_RECEIPT_SLOT3];

// [269..=270] reserved for future use

// 271 is the loader swap key for devices that have swap

// 32 bytes reserved to configure/set up clock scrambling
pub const CLOCK_SCRAMBLE_PARAMS: SlotIndex = SlotIndex::Data(272, PartitionAccess::Open, RwPerms::ReadWrite);

// 32 bytes reserved for ATE test operations. Data is read back by the ATE at this location via JTAG.
pub const ATE_RESERVED: SlotIndex = SlotIndex::Data(273, PartitionAccess::Open, RwPerms::ReadWrite);

// [274..=383] available for future data slot usage - in particular opentitan provisioning block (2k of data)
// can fit in this with some margin.

// Notes on defining the boot0 IFR region.
// RISC-V boot0 region start is defined by IFR slot 6, bits [55:48]
// RISC-V boot1 region start (which is boot0 end) is defined by IFR slot 6, bits[47:40]
// These bits are compared against address [21:14], which means the RRAM region is
// sub-dividable into 256 16k blocks
//   ** Boot0 should be from 0x0 - 0x2_0000 (128k reserved as R/O data)
//    - "start" bits [55:48] == 0x00
//    - "end" bits [47:40] == 0x08
//
// Furthermore, IFR slot 0x14, bits [127:120] should have 0x3a in it to enforce write disable on boot0

/// Application slots are available to end users for use, and not reserved for OS or security
pub const APPLICATION: SlotIndex = SlotIndex::DataRange(384..1920, PartitionAccess::Open, RwPerms::ReadWrite);

// ===================== ONE-WAY COUNTER SLOT CHECKER =====================
//
// Below is testing code generated by Claude Opus 4.8-high to make sure there are no collisions
// in the slot map. The idea is to have a build-time check that makes sure everything is consistent.
//
// Pure `no_std`, no test harness, no host toolchain: the `const _: () = assert!(...)` items
// at the bottom are evaluated during the normal cross-compile, so a bad slot map breaks the
// firmware build directly.
//
// Const panics cannot format, so the messages here are static. When one fires, run the
// host-side `owc-check` binary, which walks these same `pub` tables and reports exactly
// which claims collided.
//
// Footprint: these are `const` items, not `static`. Unreferenced consts emit no code or
// data into the firmware image, and the const fns are only ever called in const context.
//
// Verified to compile on rustc 1.75, edition 2021, `#![no_std]`.

/// Total number of one-way counters in the array.
pub const OWC_TOTAL_SLOTS: usize = 256;
/// Slots below this are reserved for bootloader/xous use; at and above are user application slots.
pub const OWC_BOOT_SLOTS: usize = 128;

/// A claim on a contiguous run of one-way counter slots.
pub struct OwcClaim {
    pub name: &'static str,
    pub first: usize,
    pub count: usize,
}

const fn claim(name: &'static str, first: usize, count: usize) -> OwcClaim { OwcClaim { name, first, count } }

/// Every slot claim in the boot region.
///
/// ADD AN ENTRY HERE whenever you allocate a slot. A constant that is not listed is
/// invisible to these checks and can be silently aliased by a later allocation.
//
// Macro-generated encodings register themselves via `<T as OneWayEncoding>::CLAIM`, which is
// derived from the `#[offset = N]` attribute. Editing that attribute alone is enough to trip
// the overlap assertion below; there is no literal here to drift out of sync.
pub const OWC_MAP: &[OwcClaim] = &[
    <UsbDefaultSpeed as OneWayEncoding>::CLAIM,
    claim("REQUIRE_PQ", REQUIRE_PQ, 1),
    claim("PQ_LOADER_REVOCATION_DUPE", PQ_LOADER_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    claim("PQ_BOOT1_REVOCATION_DUPE", PQ_BOOT1_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    claim("PQ_BOOT0_REVOCATION_DUPE", PQ_BOOT0_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    claim("PQ_LOADER_REVOCATION", PQ_LOADER_REVOCATION_OFFSET, PUBKEY_SLOTS),
    claim("PQ_BOOT1_REVOCATION", PQ_BOOT1_REVOCATION_OFFSET, PUBKEY_SLOTS),
    claim("PQ_BOOT0_REVOCATION", PQ_BOOT0_REVOCATION_OFFSET, PUBKEY_SLOTS),
    claim("REQUIRE_PQ_DUPE", REQUIRE_PQ_DUPE, 1),
    claim("EARLY_BOOT_COUNT", EARLY_BOOT_COUNT, 1),
    claim("RESERVED0_ANTI_ROLLBACK", RESERVED0_ANTI_ROLLBACK, 1),
    claim("RESERVED1_ANTI_ROLLBACK", RESERVED1_ANTI_ROLLBACK, 1),
    claim("BOOT0_ANTI_ROLLBACK", BOOT0_ANTI_ROLLBACK, 1),
    claim("BOOT1_ANTI_ROLLBACK", BOOT1_ANTI_ROLLBACK, 1),
    claim("LOADER_ANTI_ROLLBACK", LOADER_ANTI_ROLLBACK, 1),
    claim("KERNEL_ANTI_ROLLBACK", KERNEL_ANTI_ROLLBACK, 1),
    claim("SWAP_ANTI_ROLLBACK", SWAP_ANTI_ROLLBACK, 1),
    claim("APP_ANTI_ROLLBACK", APP_ANTI_ROLLBACK, 1),
    claim("BAREMETAL_ANTI_ROLLBACK", BAREMETAL_ANTI_ROLLBACK, 1),
    claim("PARANOID_MODE", PARANOID_MODE, 1),
    claim("POSSIBLE_ATTACKS", POSSIBLE_ATTACKS, 1),
    claim("PARANOID_MODE_DUPE", PARANOID_MODE_DUPE, 1),
    claim("LOADER_REVOCATION_DUPE", LOADER_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    claim("BOOT1_REVOCATION_DUPE", BOOT1_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    claim("BOOT0_REVOCATION_DUPE", BOOT0_REVOCATION_DUPE_OFFSET, PUBKEY_SLOTS),
    <BootWaitCoding as OneWayEncoding>::CLAIM,
    <BoardTypeCoding as OneWayEncoding>::CLAIM,
    <AltBootCoding as OneWayEncoding>::CLAIM,
    claim("CP_BOOT_SETUP_DONE", CP_BOOT_SETUP_DONE, 1),
    claim("IN_SYSTEM_BOOT_SETUP_DONE", IN_SYSTEM_BOOT_SETUP_DONE, 1),
    claim("DEVELOPER_MODE", DEVELOPER_MODE, 1),
    claim("OEM_MODE", OEM_MODE, 1),
    claim("BOOT0_PUBKEY_FAIL", BOOT0_PUBKEY_FAIL, 1),
    claim("BOOT1_PUBKEY_FAIL", BOOT1_PUBKEY_FAIL, 1),
    <ExternalIdentifiers as OneWayEncoding>::CLAIM,
    claim("LOADER_REVOCATION", LOADER_REVOCATION_OFFSET, PUBKEY_SLOTS),
    claim("BOOT1_REVOCATION", BOOT1_REVOCATION_OFFSET, PUBKEY_SLOTS),
    claim("BOOT0_REVOCATION", BOOT0_REVOCATION_OFFSET, PUBKEY_SLOTS),
];

/// A primary revocation block paired with its hardened duplicate.
pub struct DupePair {
    pub name: &'static str,
    pub primary: usize,
    pub dupe: usize,
    pub distance: usize,
}

const fn dupe_pair(name: &'static str, primary: usize, dupe: usize, distance: usize) -> DupePair {
    DupePair { name, primary, dupe, distance }
}

/// Every (primary, duplicate) revocation pair, with the distance constant that
/// `validate_image()` / `pq_checks()` will actually use to reach the duplicate.
///
/// This is the invariant that matters most: both checkers index the duplicate as
/// `offset + i - DUPE_DISTANCE`, which is only correct if one shared distance constant
/// holds for *all three* partitions at once. Nothing else in the tree enforces that, and
/// getting it wrong reads an unrelated counter rather than failing loudly.
pub const OWC_DUPE_PAIRS: &[DupePair] = &[
    dupe_pair("loader", LOADER_REVOCATION_OFFSET, LOADER_REVOCATION_DUPE_OFFSET, REVOCATION_DUPE_DISTANCE),
    dupe_pair("boot1", BOOT1_REVOCATION_OFFSET, BOOT1_REVOCATION_DUPE_OFFSET, REVOCATION_DUPE_DISTANCE),
    dupe_pair("boot0", BOOT0_REVOCATION_OFFSET, BOOT0_REVOCATION_DUPE_OFFSET, REVOCATION_DUPE_DISTANCE),
    dupe_pair(
        "pq_loader",
        PQ_LOADER_REVOCATION_OFFSET,
        PQ_LOADER_REVOCATION_DUPE_OFFSET,
        PQ_REVOCATION_DUPE_DISTANCE,
    ),
    dupe_pair(
        "pq_boot1",
        PQ_BOOT1_REVOCATION_OFFSET,
        PQ_BOOT1_REVOCATION_DUPE_OFFSET,
        PQ_REVOCATION_DUPE_DISTANCE,
    ),
    dupe_pair(
        "pq_boot0",
        PQ_BOOT0_REVOCATION_OFFSET,
        PQ_BOOT0_REVOCATION_DUPE_OFFSET,
        PQ_REVOCATION_DUPE_DISTANCE,
    ),
];

/// Result codes. Const panics carry no data, so separate assertions on distinct codes are
/// what let the build tell you *which* kind of mistake you made.
pub const OWC_OK: usize = 0;
pub const OWC_ERR_ZERO_COUNT: usize = 1;
pub const OWC_ERR_OUT_OF_RANGE: usize = 2;
pub const OWC_ERR_OVERLAP: usize = 3;
pub const OWC_ERR_SPILLS_INTO_USER_REGION: usize = 4;

/// Paint every claimed slot onto a bitmap; a second claim on a painted slot is an overlap.
/// The bitmap approach also picks up zero-length runs (`PUBKEY_SLOTS == 0`), runs that fall
/// off the end of the array, and boot claims that reach into user application space.
pub const fn check_owc_map(map: &[OwcClaim]) -> usize {
    let mut used = [false; OWC_TOTAL_SLOTS];
    let mut i = 0;
    while i < map.len() {
        let first = map[i].first;
        let count = map[i].count;

        if count == 0 {
            return OWC_ERR_ZERO_COUNT;
        }
        // no wraparound, and stays inside the physical array
        if first >= OWC_TOTAL_SLOTS || count > OWC_TOTAL_SLOTS - first {
            return OWC_ERR_OUT_OF_RANGE;
        }
        // boot-region claims must not reach into the user application region
        if first + count > OWC_BOOT_SLOTS {
            return OWC_ERR_SPILLS_INTO_USER_REGION;
        }

        let mut s = first;
        while s < first + count {
            if used[s] {
                return OWC_ERR_OVERLAP;
            }
            used[s] = true;
            s += 1;
        }
        i += 1;
    }
    OWC_OK
}

/// Duplicates must sit exactly `distance` below their primary. Placing one above underflows
/// the subtraction in `hardened_get2()`.
pub const fn check_dupe_pairs(pairs: &[DupePair]) -> bool {
    let mut i = 0;
    while i < pairs.len() {
        let primary = pairs[i].primary;
        let dupe = pairs[i].dupe;
        let distance = pairs[i].distance;

        if dupe >= primary {
            return false;
        }
        if primary - distance != dupe {
            return false;
        }
        i += 1;
    }
    true
}

// ---- compile-time enforcement; no harness, no host toolchain, fires on the build ----

const _: () = assert!(
    check_owc_map(OWC_MAP) != OWC_ERR_ZERO_COUNT,
    "OWC slot map: a claim has zero length (is PUBKEY_SLOTS zero?)"
);
const _: () = assert!(
    check_owc_map(OWC_MAP) != OWC_ERR_OUT_OF_RANGE,
    "OWC slot map: a claim runs past the end of the counter array"
);
const _: () = assert!(
    check_owc_map(OWC_MAP) != OWC_ERR_SPILLS_INTO_USER_REGION,
    "OWC slot map: a boot-region claim spills into the user application region (>= 128)"
);
const _: () = assert!(
    check_owc_map(OWC_MAP) != OWC_ERR_OVERLAP,
    "OWC slot map: two claims overlap - run `cargo run -p owc-check` for the offending pair"
);
const _: () = assert!(
    check_dupe_pairs(OWC_DUPE_PAIRS),
    "OWC slot map: a revocation duplicate is not exactly DUPE_DISTANCE below its primary"
);
