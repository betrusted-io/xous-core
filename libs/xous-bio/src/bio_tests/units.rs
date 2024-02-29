use super::report_api;
use crate::*;

pub fn hello_world() {
    report_api(0x0510_0000);
    // map the instruction memory
    let imem_slice = unsafe {
        core::slice::from_raw_parts_mut(
            utralib::generated::HW_BIO_RAM_MEM as *mut u32,
            utralib::generated::HW_BIO_RAM_MEM_LEN
        )
    };

    let simple_test_ptr = simple_test as *const u8;
    let simple_test_slice = unsafe {
        core::slice::from_raw_parts(simple_test_ptr,
            (simple_test_end as *const u8) as usize - simple_test_ptr as usize
        )
    };
    // copy code to reset vector for 0th machine
    for (i, chunk) in simple_test_slice.chunks(4).enumerate() {
        if chunk.len() == 4 {
            let word: u32 = u32::from_le_bytes(chunk.try_into().unwrap());
            imem_slice[i] = word;
            report_api(i as u32);
            report_api(word);
        } else {
            let mut ragged_word = 0;
            for (j, &b) in chunk.iter().enumerate() {
                ragged_word |= (b as u32) << (4 - chunk.len() + j);
            }
            imem_slice[i] = ragged_word;
        }
    }
    // configure & run the 0th machine
    let mut bio = CSR::new(utra::bio::HW_BIO_BASE as *mut u32);
    // /16 clock
    bio.wo(utra::bio::SFR_QDIV0, 0x10_0000);
    // start the machine
    bio.wo(utra::bio::SFR_CTRL, 0x111);
    report_api(0x0510_0001);
}

pub unsafe fn simple_test() {
    core::arch::asm!(
        "0:",
        "nop",
        "j 0b",
        "nop"
    );
}
// this marks the "end address" of simple_test
pub unsafe fn simple_test_end() {
    core::arch::asm!(
        "nop"
    );

}