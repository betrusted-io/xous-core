use bitbybit::*;
#[cfg(feature = "std")]
use xous::MemoryRange;

pub const KEYSEL_START: usize = 0x603F_0000;
pub const DATASEL_START: usize = 0x603E_0000;
pub const ACRAM_DATASLOT_START: usize = 0x603D_C000;
pub const ACRAM_KEYSLOT_START: usize = 0x603D_E000;
// pub const ACRAM_GKEYSLOT_START: usize = 0x603D_E400; // This is mentioned in the docs but I don't see it in
// the code?
pub const ONEWAY_START: usize = 0x603D_A000;
pub const ONEWAY2_START: usize = 0x603D_B000;
pub const ONEWAY_LEN: usize = 2048; // in u32-sized elements
pub const CODESEL_END: usize = 0x603D_A000;

#[bitfield(u32)]
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
}

#[bitfield(u32)]
pub struct KeySlotAccess {
    #[bits(24..=31, rw)]
    akey_id: u8,
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

impl KeySlotAccess {
    // This method is only valid in no-std currently. Not sure if there is even meaning for us
    // to access this in the Xous environment, as this is primarily a secure boot construct
    #[cfg(not(feature = "std"))]
    pub fn get_entry(slot: usize) -> Self {
        let slot_array =
            unsafe { core::slice::from_raw_parts(ACRAM_KEYSLOT_START as *const KeySlotAccess, 2048) };
        slot_array[slot]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OneWayErr {
    OutOfBounds,
    IncFail,
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
                xous::MemoryAddress::new(ONEWAY2_START),
                None,
                ONEWAY_LEN * size_of::<u32>(),
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
        if offset < ONEWAY_LEN {
            // safety: only safe because the pointer is length-checked
            // we use this form to access the array because we need to read_volatile()
            Ok(unsafe { base.add(offset).read_volatile() })
        } else {
            Err(OneWayErr::OutOfBounds)
        }
    }

    pub fn inc(&self, offset: usize) -> Result<(), OneWayErr> {
        #[cfg(not(feature = "std"))]
        let base = ONEWAY_START as *mut u32;
        #[cfg(feature = "std")]
        let base = self.mapping.as_mut_ptr() as *mut u32;

        if offset < ONEWAY_LEN {
            let starting_value = self.get(offset).unwrap(); // offset is already checked
            // this will cause the increment in hardware
            unsafe { base.add(offset).write_volatile(0) }
            crate::cache_flush();
            let ending_value = self.get(offset).unwrap();
            // if the increment didn't happen, we may have experienced wear-out on the line
            // it's only good for 10k increments
            if ending_value != starting_value + 1 { Err(OneWayErr::IncFail) } else { Ok(()) }
        } else {
            Err(OneWayErr::OutOfBounds)
        }
    }
}
