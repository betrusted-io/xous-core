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
    let mv_test_code = fn_to_slice(multiverse_test, multiverse_test_endcap);
    bio_ss.load_code(mv_test_code, 0);

    // configure & run the 0th machine
    // /32 clock
    bio_ss.bio.wo(utra::bio::SFR_QDIV0, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV1, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV2, 0x20_0000);
    bio_ss.bio.wo(utra::bio::SFR_QDIV3, 0x20_0000);
    // start all the machines, all at once
    bio_ss.bio.wo(utra::bio::SFR_CTRL, 0xfff);
    report_api(0x1310_10FF);
}

pub unsafe fn multiverse_test() {
    core::arch::asm!(
        // reset vectors for each machine
        "j 0f",
        "nop",
        "j 1f",
        "nop",
        "j 2f",
        "nop",
        "j 3f",
        "nop",
        // mach 0 code
        "0:",
        "li   x26, 0xFF",
        "add  x1, zero, 0x10",
        "4:",
        "add  x1, x1, 0x1",
        "mv   x21, x1",
        "mv   x20, zero",
        "j 4b",
        // mach 1 code
        "1:",
        "li   x26, 0xFF00",
        "add  x1, zero, 0x20",
        "5:",
        "add  x1, x1, 0x1",
        "slli x21, x1, 8",
        "mv   x20, zero",
        "j 5b",
        // mach 2 code
        "2:",
        "li   x26, 0xFF0000",
        "add  x1, zero, 0x20",
        "6:",
        "add  x1, x1, 0x1",
        "slli x21, x1, 16",
        "mv   x20, zero",
        "j 6b",
        // mach 3 code
        "3:",
        "li   x26, 0xFF0000",
        "add  x1, zero, 0x20",
        "7:",
        "add  x1, x1, 0x1",
        "slli x21, x1, 24",
        "mv   x20, zero",
        "j 7b",
    );
}
// this marks the "end address" of simple_test
pub unsafe fn multiverse_test_endcap() {
    core::arch::asm!(
        "nop"
    );

}
