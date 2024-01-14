pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;

// location of kernel, as offset from the base of ReRAM. This needs to match up with what is in link.x.
pub const KERNEL_OFFSET: usize = 0x9000;

#[cfg(feature = "platform-tests")]
pub mod duart {
    pub const UART_DOUT: utralib::Register = utralib::Register::new(0, 0xff);
    pub const UART_DOUT_DOUT: utralib::Field = utralib::Field::new(8, 0, UART_DOUT);
    pub const UART_CTL: utralib::Register = utralib::Register::new(1, 1);
    pub const UART_CTL_EN: utralib::Field = utralib::Field::new(1, 0, UART_CTL);
    pub const UART_BUSY: utralib::Register = utralib::Register::new(2, 1);
    pub const UART_BUSY_BUSY: utralib::Field = utralib::Field::new(1, 0, UART_BUSY);

    pub const HW_DUART_BASE: usize = 0x4004_2000;
}
#[cfg(feature = "platform-tests")]
struct Duart {
    csr: utralib::CSR<u32>,
}
#[cfg(feature = "platform-tests")]
impl Duart {
    pub fn new() -> Self {
        let mut duart_csr = utralib::CSR::new(duart::HW_DUART_BASE as *mut u32);
        duart_csr.wfo(duart::UART_CTL_EN, 1);
        Duart { csr: duart_csr }
    }

    pub fn putc(&mut self, ch: char) {
        while self.csr.rf(duart::UART_BUSY_BUSY) != 0 {
            // spin wait
        }
        // the code here bypasses a lot of checks to simulate very fast write cycles so
        // that the read waitback actually returns something other than not busy.
        // unsafe {(duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) }; // this line really ensures
        // we have to readback something, but it causes double-printing
        while unsafe { (duart::HW_DUART_BASE as *mut u32).add(2).read_volatile() } != 0 {
            // wait
        }
        unsafe { (duart::HW_DUART_BASE as *mut u32).write_volatile(ch as u32) };
    }

    pub fn puts(&mut self, s: &str) {
        for c in s.as_bytes() {
            self.putc(*c as char);
        }
    }
}
#[cfg(feature = "platform-tests")]
fn test_duart() {
    // println!("Duart test\n");
    let mut duart = Duart::new();
    loop {
        duart.puts("hello world\n");
    }
}

#[cfg(feature = "platform-tests")]
pub fn platform_tests() { test_duart(); }

#[cfg(feature = "cramium-soc")]
pub fn early_init() {
    /*
    // "actual SoC" parameters
    unsafe {
        (0x400400a0 as *mut u32).write_volatile(0x1F598); // F
        crate::println!("F: {:08x}", ((0x400400a0 as *const u32).read_volatile()));
        let poke_array: [(u32, u32, bool); 12] = [
            (0x400400a4, 0x2812, false),   //  MN
            (0x400400a8, 0x3301, false),   //  Q
            (0x40040090, 0x0032, true),  // setpll
            (0x40040014, 0x7f7f, false),  // fclk
            (0x40040018, 0x7f7f, false),  // aclk
            (0x4004001c, 0x3f3f, false),  // hclk
            (0x40040020, 0x1f1f, false),  // iclk
            (0x40040024, 0x0f0f, false),  // pclk
            (0x40040010, 0x0001, false),  // sel0
            (0x4004002c, 0x0032, true),  // setcgu
            (0x40040060, 0x0003, false),  // aclk gates
            (0x40040064, 0x0003, false),  // hclk gates
        ];
        for &(addr, dat, is_u32) in poke_array.iter() {
            let rbk = if is_u32 {
                (addr as *mut u32).write_volatile(dat);
                (addr as *const u32).read_volatile()
            } else {
                (addr as *mut u16).write_volatile(dat as u16);
                (addr as *const u16).read_volatile() as u32
            };
            if dat != rbk {
                crate::println!("{:08x}(w) != {:08x}(r)", dat, rbk);
            } else {
                crate::println!("{:08x} ok", dat);
            }
        }
    } */
    // FPGA board parameters (deals with MMCM instead of PLL)
    unsafe {
        let poke_array: [(u32, u32, bool); 9] = [
            (0x40040030, 0x0001, true),  // cgusel1
            (0x40040010, 0x0001, true),  // cgusel0
            (0x40040010, 0x0001, true),  // cgusel0
            (0x40040014, 0x007f, true),  // fdfclk
            (0x40040018, 0x007f, true),  // fdaclk
            (0x4004001c, 0x007f, true),  // fdhclk
            (0x40040020, 0x007f, true),  // fdiclk
            (0x40040024, 0x007f, true),  // fdpclk
            (0x400400a0, 0x4040, false), // pllmn FPGA
        ];
        for &(addr, dat, is_u32) in poke_array.iter() {
            let rbk = if is_u32 {
                (addr as *mut u32).write_volatile(dat);
                (addr as *const u32).read_volatile()
            } else {
                (addr as *mut u16).write_volatile(dat as u16);
                (addr as *const u16).read_volatile() as u32
            };
            if dat != rbk {
                crate::println!("{:08x}(w) != {:08x}(r)", dat, rbk);
            } else {
                crate::println!("{:08x} ok", dat);
            }
        }
    }

    use utralib::generated::*;

    // configure the UDMA UART and send a test string. This UART's settings will be
    // used as the initial console UART.
    let mut udma_ctrl = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    let iox_csr = utra::iox::HW_IOX_BASE as *mut u32;
    unsafe {
        iox_csr.add(0).write_volatile(0b00_00_00_01_01_00_00_00); // PAL AF1 on PA3/PA4
        iox_csr.add(0x1c / core::mem::size_of::<u32>()).write_volatile(0x1400); // PDH
        iox_csr.add(0x148 / core::mem::size_of::<u32>()).write_volatile(0x10); // PA4 output
        iox_csr.add(0x148 / core::mem::size_of::<u32>() + 3).write_volatile(0xffff); // PD
        iox_csr.add(0x160 / core::mem::size_of::<u32>()).write_volatile(0x8); // PA3 pullup
    }
    udma_ctrl.wo(utra::udma_ctrl::REG_CG, 1);

    let baudrate: u32 = 115200;
    let freq: u32 = 100_000_000;
    let clk_counter: u32 = (freq + baudrate / 2) / baudrate;
    let mut udma_uart = CSR::new(utra::udma_uart_0::HW_UDMA_UART_0_BASE as *mut u32);
    udma_uart.wo(utra::udma_uart_0::REG_UART_SETUP, 0x0306 | (clk_counter << 16));

    /*
    // send a test string to confirm the UART is configured
    let tx_buf = utralib::HW_IFRAM0_MEM as *mut u8;
    for i in 0..16 {
        unsafe { tx_buf.add(i).write_volatile('a' as u32 as u8 + i as u8) };
    }
    udma_uart.wo(utra::udma_uart_0::REG_TX_SADDR, tx_buf as u32);
    udma_uart.wo(utra::udma_uart_0::REG_TX_SIZE, 16);
    // send it
    udma_uart.wo(utra::udma_uart_0::REG_TX_CFG, 0x10); // EN
    // wait for it all to be done
    while udma_uart.rf(utra::udma_uart_0::REG_TX_CFG_R_TX_EN) != 0 {   }
    while (udma_uart.r(utra::udma_uart_0::REG_STATUS) & 1) != 0 {  }
    */
}
