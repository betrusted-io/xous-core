//! Current usage model is that the same process that maps ACRAM is the same
//! process that maps all of Data and Key areas. So we don't have to worry about
//! sharing protection state. However, pages of data/key area can be mapped into
//! different processes, in which case, the processes lose any ability to write
//! or directly manage the areas (it becomes effectively ROM).
//!
//! In the case of baremetal targets of course everything is mapped, so these
//! restrictions are not a concern.

use crate::acram::*;
use crate::coreuser::CoreuserId;
use crate::rram::Reram;

#[derive(PartialEq, Eq)]
pub enum AccessSettings {
    Data(DataSlotAccess),
    Key(KeySlotAccess),
}
impl AccessSettings {
    pub fn raw_u32(&self) -> u32 {
        match self {
            Self::Data(d) => d.raw_value(),
            Self::Key(d) => d.raw_value(),
        }
    }

    pub fn allows_cpu_read(&self) -> bool {
        match self {
            Self::Data(d) => !d.core_rd_dis(),
            Self::Key(d) => !d.core_rd_dis(),
        }
    }

    pub fn allows_cpu_write(&self) -> bool {
        match self {
            Self::Data(d) => !d.core_wr_dis(),
            Self::Key(d) => !d.core_wr_dis(),
        }
    }

    /// This bit is set if the slot only allows 0->1 transitions
    pub fn is_set_only(&self) -> bool {
        match self {
            Self::Data(d) => d.write_mode(),
            Self::Key(_d) => false,
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum AccessType {
    Read,
    Write,
    ReadWrite,
    None,
}
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Error {
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
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SlotIndex {
    Data(usize),
    Key(usize),
}
impl SlotIndex {
    /// Returns `OutOfBounds` error if the index specified in the slot is out of bounds.
    pub fn try_into_data_offset(&self) -> Result<usize, Error> {
        match self {
            Self::Data(index) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(*index * SLOT_ELEMENT_LEN_BYTES)
                } else {
                    Err(Error::OutOfBounds)
                }
            }
            Self::Key(index) => {
                if *index < MAX_KEY_SLOTS {
                    Ok(*index * SLOT_ELEMENT_LEN_BYTES)
                } else {
                    Err(Error::OutOfBounds)
                }
            }
        }
    }

    pub fn try_into_acl_offset(&self) -> Result<usize, Error> {
        match self {
            Self::Data(index) => {
                if *index < MAX_DATA_SLOTS {
                    Ok(*index * size_of::<u32>())
                } else {
                    Err(Error::OutOfBounds)
                }
            }
            Self::Key(index) => {
                if *index < MAX_KEY_SLOTS {
                    Ok(*index * size_of::<u32>())
                } else {
                    Err(Error::OutOfBounds)
                }
            }
        }
    }
}

pub struct SlotManager {
    data_range: xous::MemoryRange,
    data_acl_range: xous::MemoryRange,
    key_range: xous::MemoryRange,
    key_acl_range: xous::MemoryRange,
    user_id: CoreuserId,
}

impl SlotManager {
    /// Creates a handle to a new slot index
    pub fn new() -> Self {
        #[cfg(feature = "std")]
        let data_range = xous::map_memory(
            xous::MemoryAddress::new(DATA_SLOT_START),
            None,
            DATA_SLOT_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map slot range");
        #[cfg(feature = "std")]
        let data_range = xous::map_memory(
            xous::MemoryAddress::new(ACRAM_DATASLOT_START),
            None,
            ACRAM_DATASLOT_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map ACL range");
        #[cfg(feature = "std")]
        let key_range = xous::map_memory(
            xous::MemoryAddress::new(KEY_SLOT_START),
            None,
            KEY_SLOT_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map slot range");
        #[cfg(feature = "std")]
        let data_range = xous::map_memory(
            xous::MemoryAddress::new(ACRAM_KEYSLOT_START),
            None,
            ACRAM_KEYSLOT_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("Couldn't map ACL range");
        #[cfg(not(feature = "std"))]
        // safety: these ranges are valid and pre-aligned
        let data_range = unsafe { xous::MemoryRange::new(DATA_SLOT_START, DATA_SLOT_LEN).unwrap() };
        #[cfg(not(feature = "std"))]
        // safety: these ranges are valid and pre-aligned
        let data_acl_range =
            unsafe { xous::MemoryRange::new(ACRAM_DATASLOT_START, ACRAM_DATASLOT_LEN).unwrap() };
        #[cfg(not(feature = "std"))]
        // safety: these ranges are valid and pre-aligned
        let key_range = unsafe { xous::MemoryRange::new(KEY_SLOT_START, KEY_SLOT_LEN).unwrap() };
        #[cfg(not(feature = "std"))]
        // safety: these ranges are valid and pre-aligned
        let key_acl_range =
            unsafe { xous::MemoryRange::new(ACRAM_KEYSLOT_START, ACRAM_KEYSLOT_LEN).unwrap() };

        #[cfg(feature = "std")]
        let user_id = if xous::process::id() == crate::coreuser::TRUSTED_PID {
            CoreuserId::Boot1
        } else {
            CoreuserId::Fw0
        };
        // pre-boot, we are in a baremetal context and we have full access to the data slots.
        #[cfg(not(feature = "std"))]
        let user_id = CoreuserId::Boot1;

        Self { data_range, data_acl_range, key_range, key_acl_range, user_id }
    }

    pub fn read(&self, slot: &SlotIndex) -> Result<&[u8], Error> {
        // check the ACL first
        let acl = self.get_acl(slot)?;
        if self.user_id.is_accessible(&acl, &AccessType::Read) {
            let offset = slot.try_into_data_offset()?;

            // safety: the unsafes below remind us to check that all values are valid during
            // the pointer type cast. In this case, there are no invalid values for a `u8`.
            Ok(match slot {
                SlotIndex::Data(_) => unsafe {
                    let ptr: *const u8 = &self.data_range.as_slice::<u8>()[offset] as *const u8;
                    crate::println!("data loc: {:x}", ptr as usize);
                    &self.data_range.as_slice()[offset..offset + SLOT_ELEMENT_LEN_BYTES]
                },
                SlotIndex::Key(_) => unsafe {
                    &self.key_range.as_slice()[offset..offset + SLOT_ELEMENT_LEN_BYTES]
                },
            })
        } else {
            Err(Error::AccessDenied)
        }
    }

    pub fn write(
        &self,
        writer: &mut Reram,
        slot: &SlotIndex,
        value: &[u8; SLOT_ELEMENT_LEN_BYTES],
    ) -> Result<(), Error> {
        let acl = self.get_acl(slot)?;
        if self.user_id.is_accessible(&acl, &AccessType::Write) {
            let offset = slot.try_into_data_offset()?;

            let range = match slot {
                SlotIndex::Data(_) => &self.data_range,
                SlotIndex::Key(_) => &self.key_range,
            };

            writer
                .protected_write_slice(range.as_ptr() as usize + offset, value)
                .map_err(|_| Error::WriteError)?;
            crate::cache_flush();
        }

        if self.user_id.is_accessible(&acl, &AccessType::Read) {
            // read-verify only if we have read access
            let readback = self.read(slot)?;
            if readback != value { Err(Error::WriteError) } else { Ok(()) }
        } else {
            Ok(())
        }
    }

    pub fn get_acl(&self, slot: &SlotIndex) -> Result<AccessSettings, Error> {
        let offset = slot.try_into_acl_offset()?;
        // safety: the unsafe blocks here are concerned that the data types of the resulting
        // slice are all representable. In this case, all bits are valid in the final representation.
        Ok(match slot {
            SlotIndex::Data(_) => {
                AccessSettings::Data(DataSlotAccess::new_with_raw_value(u32::from_le_bytes(unsafe {
                    self.data_acl_range.as_slice()[offset..offset + size_of::<u32>()].try_into().unwrap()
                })))
            }
            SlotIndex::Key(_) => {
                AccessSettings::Key(KeySlotAccess::new_with_raw_value(u32::from_le_bytes(unsafe {
                    self.key_acl_range.as_slice()[offset..offset + size_of::<u32>()].try_into().unwrap()
                })))
            }
        })
    }

    pub fn set_acl(
        &self,
        writer: &mut Reram,
        slot: &SlotIndex,
        setting: &AccessSettings,
    ) -> Result<(), Error> {
        let offset = slot.try_into_acl_offset()?;
        // safety: the unsafe blocks here are concerned that the data types of the resulting
        // slice are all representable. In this case, all bits are valid in the final representation.
        match slot {
            SlotIndex::Data(_) => writer
                .protected_write_slice(
                    self.data_acl_range.as_ptr() as usize - utralib::HW_RERAM_MEM + offset,
                    &setting.raw_u32().to_le_bytes(),
                )
                .map(|_| ())
                .map_err(|_| Error::WriteError),
            SlotIndex::Key(_) => writer
                .protected_write_slice(
                    self.key_acl_range.as_ptr() as usize - utralib::HW_RERAM_MEM + offset,
                    &setting.raw_u32().to_le_bytes(),
                )
                .map(|_| ())
                .map_err(|_| Error::WriteError),
        }
    }
}
