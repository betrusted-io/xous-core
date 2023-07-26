pub mod i2c;
pub mod spi;
pub mod units;
pub mod adder;
pub mod nec;

#[cfg(not(any(target_os="xous", feature="rp2040")))]
static mut REPORT_ADR: Option<*mut u32> = None;

#[cfg(not(any(target_os="xous", feature="rp2040")))]
pub fn setup_reporting(rep_adr: *mut u32) {
    unsafe {REPORT_ADR = Some(rep_adr);}
}

pub fn report_api(d: u32) {
    #[cfg(not(any(target_os="xous", feature="rp2040")))]
    unsafe {
        if let Some(rep_adr) = REPORT_ADR {
            rep_adr.write_volatile(d);
        }
    }
    #[cfg(target_os="xous")]
    log::info!("report: 0x{:x}", d);
    #[cfg(feature="rp2040")]
    defmt::info!("report: 0x{:x}", d);
}

pub fn pio_tests() {
    units::instruction_tests();
    units::corner_cases();
    units::register_tests();
    units::restart_imm_test();
    units::fifo_join_test();
    units::sticky_test();
    adder::adder_test();
    nec::nec_ir_loopback_test();
    i2c::i2c_test();
    spi::spi_test();
}
