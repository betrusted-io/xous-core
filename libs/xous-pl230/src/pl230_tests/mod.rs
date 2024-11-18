pub mod units;

pub fn report_api(desc: &str, d: u32) {
    #[cfg(feature = "baremetal")]
    {
        use core::fmt::Write;
        let mut uart = crate::debug::Uart {};
        writeln!(uart, "pl230: [{}] 0x{:x}\r", desc, d).ok();
    }
    #[cfg(not(feature = "baremetal"))]
    log::info!("pl230: [{}] 0x{:x}", desc, d);
}

pub fn pl230_tests() {
    let mut pl230 = crate::Pl230::new();
    units::basic_tests(&mut pl230);
    #[cfg(feature = "pio")]
    units::pio_test(&mut pl230);
}
