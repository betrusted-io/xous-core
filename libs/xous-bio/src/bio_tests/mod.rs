pub mod units;

#[cfg(not(any(target_os = "xous")))]
mod duart {
    pub const UART_DOUT: utralib::Register = utralib::Register::new(0, 0xff);
    pub const UART_DOUT_DOUT: utralib::Field = utralib::Field::new(8, 0, UART_DOUT);
    pub const UART_CTL: utralib::Register = utralib::Register::new(1, 1);
    pub const UART_CTL_EN: utralib::Field = utralib::Field::new(1, 0, UART_CTL);
    pub const UART_BUSY: utralib::Register = utralib::Register::new(2, 1);
    pub const UART_BUSY_BUSY: utralib::Field = utralib::Field::new(1, 0, UART_BUSY);

    pub const HW_DUART_BASE: usize = 0x4004_2000;
}

#[cfg(not(any(target_os = "xous")))]
use utralib::CSR;
#[cfg(not(any(target_os = "xous")))]
pub struct Uart {}
#[cfg(not(any(target_os = "xous")))]
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

#[cfg(not(any(target_os = "xous")))]
pub fn setup_reporting(_rep_adr: *mut u32) {}

pub fn report_api(d: u32) {
    #[cfg(not(any(target_os = "xous")))]
    {
        let mut uart = Uart {};
        uart.print_hex_word(d);
        uart.newline();
    }
    #[cfg(target_os = "xous")]
    log::info!("report: 0x{:x}", d);
}

pub fn bio_tests() {
    report_api(crate::get_id());

    units::hello_world();
    units::hello_multiverse();
    units::fifo_basic();
}

// Test plan:
// Unit tests:
//   -[x] Basic FIFO stall test. Two cores writing to each other to unlock.
//   -[ ] Host FIFO stall on empty test. Core stalls until host provides data.
//   -[ ] Host FIFO stall on full test. Core stall until host reads data.
//   -[ ] GPIO input path test
//   -[ ] GPIO direction control test
//   -[ ] FIFO level trigger test - eq, gt, lt on various channels, at various fullness levels
//   -[ ] Stall on event - register bit test, between cores
//   -[ ] Stall on event - register bit test, to host
//   -[ ] Stall on event - FIFO level test
//   -[ ] Host IRQ generation test - some combination with event tests above to confirm IRQ generation
//   -[ ] Extclk as x20 stall source
//   -[ ] Check Extclk:gpio pin mapping (make sure bit ordering is not swapped)
//   -[ ] Core ID read test
//   -[ ] Core aclk counter test
// Application tests:
//   -[ ] SPI loopback test - implement using extclk as spi clk for input
//   -[ ] I2C loopback test
//   -[ ] IR controller test
//   -[ ] Arithmetic test, utilizing load/store on core 0
//   -[ ] Arithmetic test, without store, cores 1-3
//   -[ ] DMA transfer test -- might have to wait until full-chip integration to get MDMA core?
