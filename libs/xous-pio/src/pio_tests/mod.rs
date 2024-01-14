pub mod adder;
pub mod i2c;
pub mod nec;
pub mod spi;
pub mod units;

#[cfg(not(any(target_os = "xous", feature = "rp2040")))]
mod duart {
    pub const UART_DOUT: utralib::Register = utralib::Register::new(0, 0xff);
    pub const UART_DOUT_DOUT: utralib::Field = utralib::Field::new(8, 0, UART_DOUT);
    pub const UART_CTL: utralib::Register = utralib::Register::new(1, 1);
    pub const UART_CTL_EN: utralib::Field = utralib::Field::new(1, 0, UART_CTL);
    pub const UART_BUSY: utralib::Register = utralib::Register::new(2, 1);
    pub const UART_BUSY_BUSY: utralib::Field = utralib::Field::new(1, 0, UART_BUSY);

    pub const HW_DUART_BASE: usize = 0x4004_2000;
}

#[cfg(not(any(target_os = "xous", feature = "rp2040")))]
use utralib::CSR;
#[cfg(not(any(target_os = "xous", feature = "rp2040")))]
pub struct Uart {}
#[cfg(not(any(target_os = "xous", feature = "rp2040")))]
impl Uart {
    fn put_digit(&mut self, d: u8) {
        let nyb = d & 0xF;
        let c = if nyb < 10 { nyb + 0x30 } else { nyb + 0x61 - 10 };
        assert!(c >= 0x30, "conversion failed!");
        self.putc(c);
    }

    pub fn put_hex(&mut self, c: u8) {
        self.put_digit(c >> 4);
        self.put_digit(c & 0xF);
    }

    pub fn newline(&mut self) { self.putc(0xd); }

    pub fn print_hex_word(&mut self, word: u32) {
        for &byte in word.to_be_bytes().iter() {
            self.put_hex(byte);
        }
    }

    pub fn putc(&self, c: u8) {
        let base = duart::HW_DUART_BASE as *mut u32;
        let mut uart = CSR::new(base);

        if uart.rf(duart::UART_CTL_EN) == 0 {
            uart.wfo(duart::UART_CTL_EN, 1);
        }
        while uart.rf(duart::UART_BUSY_BUSY) != 0 {
            // spin wait
        }
        uart.wfo(duart::UART_DOUT_DOUT, c as u32);

        #[cfg(feature = "arty")]
        self.putc_litex(c);
    }

    pub fn tiny_write_str(&mut self, s: &str) {
        for c in s.bytes() {
            self.putc(c);
        }
    }
}

#[cfg(not(any(target_os = "xous", feature = "rp2040")))]
pub fn setup_reporting(_rep_adr: *mut u32) {}

pub fn report_api(d: u32) {
    #[cfg(not(any(target_os = "xous", feature = "rp2040")))]
    {
        let mut uart = Uart {};
        uart.print_hex_word(d);
        uart.newline();
    }
    #[cfg(target_os = "xous")]
    log::info!("report: 0x{:x}", d);
    #[cfg(feature = "rp2040")]
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
