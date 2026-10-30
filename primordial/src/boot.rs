//! Common boot scratch setup shared by the hardware KAT demos.
//!
//! The `bao1x-primordial` image is built through the baremetal flow, which compresses
//! `.data` into a "statics" poke table in front of the code (right after the signature
//! block). Nothing in the runtime works reliably until that table has been applied and
//! `.data` has been zero-cleared, so every demo entry point calls [`setup`] first.

use bao1x_api::signatures::SIGBLOCK_LEN;

pub const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = crate::asm::RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

pub fn setup() {
    const STATICS_LOC: usize = bao1x_api::BOOT0_START + SIGBLOCK_LEN;

    let statics_in_rom: &bao1x_api::StaticsInRom =
        unsafe { (STATICS_LOC as *const bao1x_api::StaticsInRom).as_ref().unwrap() };
    assert!(statics_in_rom.version == bao1x_api::STATICS_IN_ROM_VERSION, "Can't find valid statics table");

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    // Safety: only safe if the values computed by the loader are correct.
    unsafe {
        let data_ptr = statics_in_rom.data_origin as *mut u32;
        for i in 0..statics_in_rom.data_size_bytes as usize / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for &(offset, data) in &statics_in_rom.poke_table[..statics_in_rom.valid_pokes as usize] {
            data_ptr
                .add(u16::from_le_bytes(offset) as usize / size_of::<u32>())
                .write_volatile(u32::from_le_bytes(data));
        }
    }
    unsafe {
        crate::ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}
