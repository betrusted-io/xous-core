//! Current usage model is that the same process that maps ACRAM is the same
//! process that maps all of Data and Key areas. So we don't have to worry about
//! sharing protection state. However, pages of data/key area can be mapped into
//! different processes, in which case, the processes lose any ability to write
//! or directly manage the areas (it becomes effectively ROM).
//!
//! In the case of baremetal targets of course everything is mapped, so these
//! restrictions are not a concern.

use bao1x_api::OneWayEncoding;
use bao1x_api::offsets::*;
#[cfg(feature = "std")]
use xous::MemoryRange;

use crate::coreuser::CoreuserId;
use crate::rram::Reram;

pub const ONEWAY_START: usize = 0x603D_A000; // page with 128 counters
pub const ONEWAY2_START: usize = 0x603D_B000; // page with another 128 counters
const ONEWAY_LEN: usize = 256 / 8; // in bytes
const COUNTER_STRIDE_U32: usize = ONEWAY_LEN / size_of::<u32>();
pub const MAX_ONEWAY_COUNTERS: usize = 8192 / ONEWAY_LEN;
pub const CODESEL_END: usize = 0x603D_A000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OneWayErr {
    OutOfBounds,
    IncFail,
    InvalidCoding,
}
pub struct OneWayCounter {
    #[cfg(feature = "std")]
    mapping: MemoryRange,
}
impl OneWayCounter {
    pub fn new() -> Self {
        #[cfg(not(feature = "std"))]
        let ret = OneWayCounter {};

        #[cfg(feature = "std")]
        let ret = OneWayCounter {
            mapping: xous::syscall::map_memory(
                xous::MemoryAddress::new(ONEWAY_START),
                None,
                ONEWAY_LEN * MAX_ONEWAY_COUNTERS,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map oneway range"),
        };

        ret
    }

    pub fn get(&self, offset: usize) -> Result<u32, OneWayErr> {
        #[cfg(not(feature = "std"))]
        let base = ONEWAY_START as *const u32;
        #[cfg(feature = "std")]
        let base = self.mapping.as_ptr() as *const u32;
        if offset < MAX_ONEWAY_COUNTERS {
            // safety: only safe because the pointer is length-checked
            // we use this form to access the array because we need to read_volatile()
            Ok(unsafe { base.add(offset * COUNTER_STRIDE_U32).read_volatile() })
        } else {
            Err(OneWayErr::OutOfBounds)
        }
    }

    /// Marked as `unsafe` because the offset needs to be correct. It's recommended to use
    /// `inc_coded()` where possible. This function is necessary for the cases that don't
    /// fit into the `encode_oneway` mechanism, e.g. key revocations, etc.
    ///
    /// All you have to do to be safe is no be super-sure you got the offset right.
    pub unsafe fn inc(&self, offset: usize) -> Result<(), OneWayErr> {
        #[cfg(not(feature = "std"))]
        let base = ONEWAY_START as *mut u32;
        #[cfg(feature = "std")]
        let base = self.mapping.as_mut_ptr() as *mut u32;

        if offset < MAX_ONEWAY_COUNTERS {
            let starting_value = self.get(offset).unwrap(); // offset is already checked
            // this will cause the increment in hardware
            unsafe { base.add(offset * COUNTER_STRIDE_U32).write_volatile(0) }
            crate::cache_flush();
            let ending_value = self.get(offset).unwrap();
            // if the increment didn't happen, we may have experienced wear-out on the line
            // it's only good for 10k increments
            if ending_value != starting_value + 1 { Err(OneWayErr::IncFail) } else { Ok(()) }
        } else {
            Err(OneWayErr::OutOfBounds)
        }
    }

    /// Automatically increments the correct slot based on the OFFSET encoded in the definition
    pub fn inc_coded<T>(&self) -> Result<(), OneWayErr>
    where
        T: OneWayEncoding,
        T::Error: core::fmt::Debug,
    {
        #[cfg(not(feature = "std"))]
        let base = ONEWAY_START as *mut u32;
        #[cfg(feature = "std")]
        let base = self.mapping.as_mut_ptr() as *mut u32;

        let offset = T::OFFSET;
        if offset < MAX_ONEWAY_COUNTERS {
            let starting_value = self.get(offset).unwrap(); // offset is already checked
            // this will cause the increment in hardware
            unsafe { base.add(offset * COUNTER_STRIDE_U32).write_volatile(0) }
            crate::cache_flush();
            let ending_value = self.get(offset).unwrap();
            // crate::println!("ending: {} starting: {}", ending_value, starting_value);

            // if the increment didn't happen, we may have experienced wear-out on the line
            // it's only good for 10k increments
            if ending_value != starting_value + 1 { Err(OneWayErr::IncFail) } else { Ok(()) }
        } else {
            Err(OneWayErr::OutOfBounds)
        }
    }

    pub fn get_decoded<T>(&self) -> Result<T, OneWayErr>
    where
        T: OneWayEncoding,
        T: TryFrom<u32>,
        T::Error: core::fmt::Debug,
    {
        let raw = self.get(T::OFFSET)?;
        T::try_from(raw).map_err(|_| OneWayErr::InvalidCoding)
    }
}

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
        let data_acl_range = xous::map_memory(
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
        let key_acl_range = xous::map_memory(
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
        let user_id = if xous::process::id() == crate::coreuser::TRUSTED_PID as u32 {
            crate::coreuser::TRUSTED_USER
        } else {
            crate::coreuser::LEAST_TRUSTED_USER
        };
        // pre-boot, we are in a baremetal context and we have full access to the data slots.
        #[cfg(not(feature = "std"))]
        let user_id = crate::coreuser::TRUSTED_USER;

        Self { data_range, data_acl_range, key_range, key_acl_range, user_id }
    }

    pub fn read(&self, slot: &SlotIndex) -> Result<&[u8], AccessError> {
        // check the ACL first
        let acl = self.get_acl(slot)?;
        if self.user_id.is_accessible(&acl, &AccessType::Read) {
            let offset = slot.try_into_data_offset()?;

            // safety: the unsafes below remind us to check that all values are valid during
            // the pointer type cast. In this case, there are no invalid values for a `u8`.
            Ok(match slot {
                SlotIndex::Data(_, _) => unsafe {
                    let ptr: *const u8 = &self.data_range.as_slice::<u8>()[offset] as *const u8;
                    crate::println!("data loc: {:x}", ptr as usize);
                    &self.data_range.as_slice()[offset..offset + SLOT_ELEMENT_LEN_BYTES]
                },
                SlotIndex::Key(_, _) => unsafe {
                    let ptr: *const u8 = &self.data_range.as_slice::<u8>()[offset] as *const u8;
                    crate::println!("data loc: {:x}", ptr as usize);
                    &self.key_range.as_slice()[offset..offset + SLOT_ELEMENT_LEN_BYTES]
                },
                _ => return Err(AccessError::TypeError),
            })
        } else {
            Err(AccessError::AccessDenied)
        }
    }

    pub fn write(
        &self,
        writer: &mut Reram,
        slot: &SlotIndex,
        value: &[u8; SLOT_ELEMENT_LEN_BYTES],
    ) -> Result<(), AccessError> {
        let acl = self.get_acl(slot)?;
        if self.user_id.is_accessible(&acl, &AccessType::Write) {
            let offset = slot.try_into_data_offset()?;

            let range = match slot {
                SlotIndex::Data(_, _) => &self.data_range,
                SlotIndex::Key(_, _) => &self.key_range,
                _ => return Err(AccessError::TypeError),
            };

            writer
                .protected_write_slice(range.as_ptr() as usize + offset, value)
                .map_err(|_| AccessError::WriteError)?;
            crate::cache_flush();
        }

        if self.user_id.is_accessible(&acl, &AccessType::Read) {
            // read-verify only if we have read access
            let readback = self.read(slot)?;
            if readback != value { Err(AccessError::WriteError) } else { Ok(()) }
        } else {
            Ok(())
        }
    }

    pub fn get_acl(&self, slot: &SlotIndex) -> Result<AccessSettings, AccessError> {
        let offset = slot.try_into_acl_offset()?;
        // safety: the unsafe blocks here are concerned that the data types of the resulting
        // slice are all representable. In this case, all bits are valid in the final representation.
        Ok(match slot {
            SlotIndex::Data(_, _) => {
                AccessSettings::Data(DataSlotAccess::new_with_raw_value(u32::from_le_bytes(unsafe {
                    self.data_acl_range.as_slice()[offset..offset + size_of::<u32>()].try_into().unwrap()
                })))
            }
            SlotIndex::Key(_, _) => {
                AccessSettings::Key(KeySlotAccess::new_with_raw_value(u32::from_le_bytes(unsafe {
                    self.key_acl_range.as_slice()[offset..offset + size_of::<u32>()].try_into().unwrap()
                })))
            }
            _ => return Err(AccessError::TypeError),
        })
    }

    pub fn set_acl(
        &self,
        writer: &mut Reram,
        slot: &SlotIndex,
        setting: &AccessSettings,
    ) -> Result<(), AccessError> {
        let offset = slot.try_into_acl_offset()?;
        // safety: the unsafe blocks here are concerned that the data types of the resulting
        // slice are all representable. In this case, all bits are valid in the final representation.
        match slot {
            SlotIndex::Data(_, _) => writer
                .protected_write_slice(
                    self.data_acl_range.as_ptr() as usize - utralib::HW_RERAM_MEM + offset,
                    &setting.raw_u32().to_le_bytes(),
                )
                .map(|_| ())
                .map_err(|_| AccessError::WriteError),
            SlotIndex::Key(_, _) => writer
                .protected_write_slice(
                    self.key_acl_range.as_ptr() as usize - utralib::HW_RERAM_MEM + offset,
                    &setting.raw_u32().to_le_bytes(),
                )
                .map(|_| ())
                .map_err(|_| AccessError::WriteError),
            _ => return Err(AccessError::TypeError),
        }
    }
}
