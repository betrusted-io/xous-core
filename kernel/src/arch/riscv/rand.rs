static mut LFSR: u32 = 0xace1u32;

fn move_lfsr(mut lfsr: u32) -> u32 {
    lfsr ^= lfsr >> 7;
    lfsr ^= lfsr << 9;
    lfsr ^= lfsr >> 13;
    lfsr
}

pub fn get_u32() -> u32 {
    // The kernel is currently single-threaded, so this is a valid operation.
    unsafe {
        LFSR = move_lfsr(LFSR);
        LFSR
    }
}
