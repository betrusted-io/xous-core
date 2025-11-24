pub mod baosec;
pub mod common;
pub use common::*;
pub mod dabao;
use core::ops::Range;

use arbitrary_int::{Number, u4};
use bitbybit::bitfield;

pub const DATA_SLOT_START: usize = 0x603E_0000;
pub const DATA_SLOT_LEN: usize = 0x1_0000;
pub const MAX_DATA_SLOTS: usize = DATA_SLOT_LEN / SLOT_ELEMENT_LEN_BYTES;
pub const SLOT_ELEMENT_LEN_BYTES: usize = 256 / 8;

pub const ACRAM_DATASLOT_START: usize = 0x603D_C000;
pub const ACRAM_DATASLOT_LEN: usize = 0x2000;
pub const IFR_BASE: usize = 0x6040_0000;
pub const IFR_LEN: usize = 0x400;
/// This number is special: it only appears during certain portions of the chip life cycle in
/// some configurations, so it needs to be captured during manufacturing and copied elsewhere in the array.
/// It contains the lot ID, wafer ID, and wafer x/y position of the die.
pub const IFR_CP_ID_BASE: usize = 0x6040_00A0;

pub const AO_BU_MEM: usize = 0x5030_0000;
pub const AO_BU_MEM_LEN: usize = 0x4000;

#[bitfield(u32)]
#[derive(PartialEq, Eq, Debug)]
pub struct DataSlotAccess {
    #[bit(24, rw)]
    write_mode: bool,
    #[bit(23, rw)]
    fw1: bool,
    #[bit(22, rw)]
    fw0: bool,
    #[bit(21, rw)]
    boot1: bool,
    #[bit(20, rw)]
    boot0: bool,
    #[bits(8..=15, rw)]
    seg_id: u8,
    #[bit(3, rw)]
    sce_wr_dis: bool,
    #[bit(2, rw)]
    sce_rd_dis: bool,
    #[bit(1, rw)]
    core_wr_dis: bool,
    #[bit(0, rw)]
    core_rd_dis: bool,
}

impl DataSlotAccess {
    // This method is only valid in no-std currently. Not sure if there is even meaning for us
    // to access this in the Xous environment, as this is primarily a secure boot construct
    #[cfg(not(feature = "std"))]
    pub fn get_entry(slot: usize) -> Self {
        let slot_array =
            unsafe { core::slice::from_raw_parts(ACRAM_DATASLOT_START as *const DataSlotAccess, 2048) };
        slot_array[slot]
    }

    pub fn get_partition_access(&self) -> PartitionAccess { PartitionAccess::from_raw_u32(self.raw_value()) }

    pub fn set_partition_access(&mut self, pa: &PartitionAccess) {
        *self = Self::new_with_raw_value((self.raw_value() & !(0xf << 20)) | (pa.to_raw_u4().as_u32() << 20));
    }

    pub fn get_rw_permissions(&self) -> RwPerms {
        match [self.core_rd_dis(), self.core_wr_dis()] {
            [true, true] => RwPerms::Denied,
            [false, true] => RwPerms::ReadOnly,
            [true, false] => RwPerms::WriteOnly,
            [false, false] => RwPerms::ReadWrite,
        }
    }

    pub fn set_rw_permissions(&mut self, spec: RwPerms) {
        match spec {
            RwPerms::Denied => {
                self.set_core_rd_dis(true);
                self.set_core_wr_dis(true);
            }
            RwPerms::ReadOnly => {
                self.set_core_rd_dis(false);
                self.set_core_wr_dis(true);
            }
            RwPerms::WriteOnly => {
                self.set_core_rd_dis(true);
                self.set_core_wr_dis(false);
            }
            RwPerms::ReadWrite => {
                self.set_core_rd_dis(false);
                self.set_core_wr_dis(false);
            }
            RwPerms::Unspecified => panic!("RwPerms must be specified"),
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum AccessError {
    /// Access is anticipated to be denied by the hardware. Users can attempt to still
    /// bypass the access control, but the result will either be invalid data, returned,
    /// a failed, write, or a security alarm being raised, depending on the hardware
    /// enforcement policy at play.
    AccessDenied,
    /// Returned when data is written to a slot that only supports 0->1 transitions
    /// and the provided data contains 1->0 transitions, but the 0->1 transitions
    /// were in fact correctly set
    OnlyOnes,
    /// Returned when data written did not verify
    WriteError,
    /// Returned when the wrong type of access settings are passed for setting access
    TypeError,
    /// Returned when an index request is out of valid bounds
    OutOfBounds,
    /// Returned when a buffer passed does not match the expected size
    SizeError,
    /// Returned when a set of slots that should have the same ACL don't. The value
    /// inside is the value of the first ACL in the set.
    DataAclInconsistency(DataSlotAccess),
}

/// Enum to specify read/write permissions to a given slot.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum RwPerms {
    ReadOnly,
    WriteOnly,
    ReadWrite,
    Denied,
    Unspecified,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
/// Specifies what partitions can access a given slot. Some common patterns are
/// provided, a Custom field is also provided for other odd combinations.
pub enum PartitionAccess {
    /// Open disables not just PartitionAccess but also all other security controls
    Open,
    All,
    Boot0,
    Boot1,
    Fw0,
    Fw1,
    AllBoots,
    AllFws,
    /// Convenience option for API calls and tests that don't care about this portion
    /// of the access control field
    Unspecified,
    /// Stores directly the bit pattern as should be written into the field,
    /// complete with the sense inversion where 0 == access allowed.
    Custom(u4),
}
impl PartitionAccess {
    /// Takes in a raw u32 pattern from either DataSlotAccess or KeySlotAccess and
    /// extracts the PartitionAccess code
    pub fn from_raw_u32(raw: u32) -> Self {
        // The bitfield coding is fw1:fw0:boot1:boot0 from MSB to LSB.
        let code: u4 = u4::new(((raw >> 20) & 0xF) as u8);
        match code.value() {
            0b0000 => Self::Open,
            0b1111 => Self::All,
            0b0001 => Self::Boot0,
            0b0010 => Self::Boot1,
            0b0100 => Self::Fw0,
            0b1000 => Self::Fw1,
            0b0011 => Self::AllBoots,
            0b1100 => Self::AllFws,
            _ => Self::Custom(u4::new(((raw >> 20) & 0xF) as u8)),
        }
    }

    // internal function that translates the symbolic representation into a u4 that
    // can be shifted into place.
    fn to_raw_u4(&self) -> u4 {
        match self {
            Self::Open => u4::new(0b0000 & 0xF),
            Self::All => u4::new(0b1111 & 0xF),
            Self::Boot0 => u4::new(0b0001 & 0xF),
            Self::Boot1 => u4::new(0b0010 & 0xF),
            Self::Fw0 => u4::new(0b0100 & 0xF),
            Self::Fw1 => u4::new(0b1000 & 0xF),
            Self::AllBoots => u4::new(0b0011 & 0xF),
            Self::AllFws => u4::new(0b1100 & 0xF),
            // Panic is the correct behavior here because it's a static code bug to try and use this coding in
            // this fashion.
            Self::Unspecified => panic!("Attempt to resolve an unspecified access pattern"),
            Self::Custom(f) => *f,
        }
    }
}

/// `SlotIndex` encodes the index of a given slot and the *specification* of what the access
/// rights should be for that slot. The actual enforcement is done by hardware, so if someone
/// tries to "lie" to the API by creating a SlotIndex specifier with an inaccurate `PartitionAccess`
/// spec, hardware will ignore it.
///
/// The reason the two are bundled together is that semantic priority is given to getting the spec right
/// in the constants in this crate that define the access control tables.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum SlotIndex {
    Data(usize, PartitionAccess, RwPerms),
    DataRange(Range<usize>, PartitionAccess, RwPerms),
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum SlotType {
    Data,
}
impl SlotIndex {
    pub fn get_access_spec(&self) -> (PartitionAccess, RwPerms) {
        match self {
            Self::Data(_, spec, rw) => (*spec, *rw),
            Self::DataRange(_, spec, rw) => (*spec, *rw),
        }
    }

    pub fn get_type(&self) -> SlotType {
        match self {
            Self::Data(_, _, _) | Self::DataRange(_, _, _) => SlotType::Data,
        }
    }

    pub fn get_base(&self) -> usize {
        match self {
            Self::Data(base, _, _) => *base,
            Self::DataRange(range, _, _) => range.start,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Data(_, _, _) => 1,
            Self::DataRange(range, _, _) => range.len(),
        }
    }

    /// Returns `OutOfBounds` error if the index specified in the slot is out of bounds.
    /// For ranges, returns the offset of the first element of the range.
    pub fn try_into_data_offset(&self) -> Result<usize, AccessError> {
        match self {
            Self::Data(index, _, _) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(*index * SLOT_ELEMENT_LEN_BYTES)
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
            Self::DataRange(range, _, _) => {
                let index = range.start;
                if range.end <= MAX_DATA_SLOTS {
                    Ok(index * SLOT_ELEMENT_LEN_BYTES)
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
        }
    }

    pub fn try_into_data_iter(&self) -> Result<SlotOffsetIter, AccessError> {
        match self {
            Self::Data(index, _, _) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(SlotOffsetIter::Single(core::iter::once(*index * SLOT_ELEMENT_LEN_BYTES)))
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
            Self::DataRange(range, _, _) => {
                if range.end > MAX_DATA_SLOTS {
                    return Err(AccessError::OutOfBounds);
                }
                Ok(SlotOffsetIter::Range(range.clone().map(|idx| idx * SLOT_ELEMENT_LEN_BYTES)))
            }
        }
    }

    pub fn try_into_acl_offset(&self) -> Result<usize, AccessError> {
        match self {
            Self::Data(index, _, _) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(*index * size_of::<u32>())
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
            Self::DataRange(range, _, _) => {
                if range.end <= MAX_DATA_SLOTS {
                    Ok(range.start * size_of::<u32>())
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
        }
    }

    pub fn try_into_acl_iter(&self) -> Result<SlotOffsetIter, AccessError> {
        const ACL_SIZE: usize = core::mem::size_of::<u32>();

        match self {
            Self::Data(index, _, _) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(SlotOffsetIter::Single(core::iter::once(*index * ACL_SIZE)))
                } else {
                    Err(AccessError::OutOfBounds)
                }
            }
            Self::DataRange(range, _, _) => {
                if range.end > MAX_DATA_SLOTS {
                    return Err(AccessError::OutOfBounds);
                }
                Ok(SlotOffsetIter::Range(range.clone().map(|idx| idx * ACL_SIZE)))
            }
        }
    }
}

// Custom iterator enum for zero-cost abstraction
pub enum SlotOffsetIter {
    Single(core::iter::Once<usize>),
    Range(core::iter::Map<Range<usize>, fn(usize) -> usize>),
}

impl Iterator for SlotOffsetIter {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Single(iter) => iter.next(),
            Self::Range(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Single(iter) => iter.size_hint(),
            Self::Range(iter) => iter.size_hint(),
        }
    }
}

impl ExactSizeIterator for SlotOffsetIter {
    fn len(&self) -> usize {
        match self {
            Self::Single(iter) => iter.len(),
            Self::Range(iter) => iter.len(),
        }
    }
}
