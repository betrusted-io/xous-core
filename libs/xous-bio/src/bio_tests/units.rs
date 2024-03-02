use super::report_api;
use crate::*;

pub fn hello_world() {
    report_api(0x1310_0000);
    let mut bio_ss = BioSharedState::new();
    let simple_test_code = fn_to_slice(simple_test, simple_test_endcap);
    // copy code to reset vector for 0th machine
    bio_ss.load_code(simple_test_code, 0);

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x20_0000);
    // start the machine
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x111);
    report_api(0x1310_00FF);
}

pub unsafe fn simple_test() {
    core::arch::asm!(
        "add  x1, zero, 0x10",
        "0:",
        "add  x1, x1, 0x1",
        "mv   x21, x1",
        "mv   x20, zero",
        "j 0b",
        "nop"
    );
}
// this marks the "end address" of simple_test
pub unsafe fn simple_test_endcap() {
    core::arch::asm!(
        "nop"
    );

}

pub fn hello_multiverse() {
    report_api(0x1310_1000);
    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = multiverse_code();
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV2, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV3, 0x20_0000);
    // snap GPIO outputs to the quantum
    bio_ss.bio.wo(utra::bio::SFR_CONFIG,
        bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_TO_QUANTUM, 1)
        | bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_TO_QUANTUM, 2) // arbitrary choice, they should all be the same
    );
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xfff);
    report_api(0x1310_10FF);
}

bio_code!(multiverse_code, MULTIVERSE_START, MULTIVERSE_END,
    // Reset vectors for each core are aligned to 4-byte boundaries
    // As long as the jump target is <2kiB from reset, this will emit
    // a C-instruction, so it needs padding with a NOP. Unfortunately,
    // I can't seem to figure out a way to force the assembler to always
    // encode as uncompressed, so, you have to be aware of the jump destination
    // for the assembler output to line up according to your expectation :(
    //
    // using 'c.j' syntax for the jump causes the assembler to emit an error,
    // but the code still compiles, so...avoiding that for now. might be a bug,
    // but I am very not interested in fixing that today.
    //
    // Also note that labels can only be numbers from 0-99, and, due to an llvm
    // bug, labels made exclusively of 0 or 1 should be avoided because they get
    // interpreted as binary numbers. dat's some jank in the tank!!
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
    // mach 0 code
    "90:",
    // x26 sets the GPIO mask
    "li   x2, 0xFF",    // load constants into r0-15 bank first
    "mv   x26, x2",     // it's not legal to do anything other than mv to x26
    "add  x1, zero, 0x10",
    "4:",
    "add  x1, x1, 0x1",
    // x21 write clobbers the GPIO bits, ANDed with mask in x26
    "mv   x21, x1",
    // x20 write causes core to wait until next sync quantum
    "mv   x20, zero",
    "j 4b",
    // mach 1 code
    "91:",
    "li   x2, 0xFF00",
    "mv   x26, x2",
    "add  x1, zero, 0x20",
    "5:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 8",
    "mv   x20, zero",
    "j 5b",
    // mach 2 code
    "92:",
    "li   x2, 0xFF0000",
    "mv   x26, x2",
    "add  x1, zero, 0x30",
    "6:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 16",
    "mv   x20, zero",
    "j 6b",
    // mach 3 code
    "93:",
    "li   x2, 0xFF000000",
    "mv   x26, x2",
    "add  x1, zero, 0x40",
    "7:",
    "add  x1, x1, 0x1",
    "slli x21, x1, 24",
    "mv   x20, zero",
    "j 7b"
);

pub fn fifo_basic() {
    report_api(0x1310_2000);
    report_api(0xf1f0_0000);

    let mut bio_ss = BioSharedState::new();
    // stop all the machines, so that code can be loaded
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0x0);
    let code = fifo_basic_code();
    bio_ss.load_code(code, 0);

    // configure & run the 0th machine
    // / 16. clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x23_BE00);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x33_1200);
    // snap GPIO outputs to the quantum
    bio_ss.bio.wo(utra::bio::SFR_CONFIG,
        bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_TO_QUANTUM, 1)
        | bio_ss.bio.ms(utra::bio::SFR_CONFIG_SNAP_TO_QUANTUM, 2) // arbitrary choice, they should all be the same
    );
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xfff);

    report_api(0x1310_20FF);
}

bio_code!(fifo_basic_code, FIFO_BASIC_START, FIFO_BASIC_END,
    "j 90f",
    "nop",
    "j 91f",
    "nop",
    "j 92f",
    "nop",
    "j 93f",
    "nop",
    // mach 0 code
    "90:",
    "li x2, 0xFFFF",
    "mv x26, x2",
    "li x1, 0x10000000",
    "li x2, 0x10000",
    "11:",
    "mv x16, x1",
    "mv x21, x17",
    "add x1, x1, x2",
    "mv x20, zero",
    "j 11b",
    // mach 1 code
    "91:",
    "li x2, 0xFFFF0000",
    "mv x26, x2",
    "li x1, 0x8000",
    "21:",
    "mv x17, x1",
    "mv x21, x16",
    "addi x1, x1, 1",
    "mv x20, zero",
    "j 21b",
    // mach 2 code
    "92:",
    "j 92b",
    // mach 3 code
    "93:",
    "j 93b",
    "nop"
);