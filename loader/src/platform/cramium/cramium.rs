pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;

// location of kernel, as offset from the base of ReRAM. This needs to match up with what is in link.x.
pub const KERNEL_OFFSET: usize = 0x9000;

#[cfg(feature="platform-tests")]
pub mod duart {
    pub const UART_DOUT: utralib::Register = utralib::Register::new(0, 0xff);
    pub const UART_DOUT_DOUT: utralib::Field = utralib::Field::new(8, 0, UART_DOUT);
    pub const UART_CTL: utralib::Register = utralib::Register::new(1, 1);
    pub const UART_CTL_EN: utralib::Field = utralib::Field::new(1, 0, UART_CTL);
    pub const UART_BUSY: utralib::Register = utralib::Register::new(2, 1);
    pub const UART_BUSY_BUSY: utralib::Field = utralib::Field::new(1, 0, UART_BUSY);

    pub const HW_DUART_BASE: usize = 0x4000_1000;
}
#[cfg(feature="platform-tests")]
struct Duart {
    csr: utralib::CSR::<u32>,
}
#[cfg(feature="platform-tests")]
impl Duart {
    pub fn new() -> Self {
        let mut duart_csr = utralib::CSR::new(duart::HW_DUART_BASE as *mut u32);
        duart_csr.wfo(duart::UART_CTL_EN, 1);
        Duart {
            csr: duart_csr,
        }
    }
    pub fn putc(&mut self, ch: char) {
        while self.csr.rf(duart::UART_BUSY_BUSY) != 0 {
            // spin wait
        }
        // the code here bypasses a lot of checks to simulate very fast write cycles so
        // that the read waitback actually returns something other than not busy.
        // unsafe {(duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) }; // this line really ensures we have to readback something, but it causes double-printing
        while unsafe{(duart::HW_DUART_BASE as *mut u32).add(2).read_volatile()} != 0 {
            // wait
        }
        unsafe {(duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) };
    }
    pub fn puts(&mut self, s: &str) {
        for c in s.as_bytes() {
            self.putc(*c as char);
        }
    }
}
#[cfg(feature="platform-tests")]
fn test_duart() {
    // println!("Duart test\n");
    let mut duart = Duart::new();
    loop {
        duart.puts("hello world\n");
    }
}

#[cfg(feature="platform-tests")]
pub fn platform_tests() {
    test_duart();
}