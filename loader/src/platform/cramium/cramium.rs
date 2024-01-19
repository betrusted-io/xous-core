use cramium_hal::iox::{Iox, IoxDir, IoxEnable, IoxFunction, IoxPort};
use cramium_hal::udma;
use utralib::generated::*;

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;

// location of kernel, as offset from the base of ReRAM. This needs to match up with what is in link.x.
pub const KERNEL_OFFSET: usize = 0x9000;

#[cfg(feature = "cramium-soc")]
pub fn early_init() {
    // Set up the initial clocks. This is done as a "poke array" into a table of addresses.
    // Why? because this is actually how it's done for the chip verification code. We can
    // make this nicer and more abstract with register meanings down the road, if necessary,
    // but for now this actually makes it easier to maintain, because we can visually compare the
    // register settings directly againt what the designers are using in validation.
    //
    // Not all design changes have a rhyme or reason at this stage -- sometimes "it just works,
    // don't futz with it" is actually the answer that goes to production.

    /*
    // "actual SoC" parameters -- swap the comment here when silicon comes back
    // not making it a "feature" because this is a one-way gate, I don't see
    // any reason why we'd go back to using the emulator board if we have silicon.
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
    // SoC emulator board parameters (deals with MMCM instead of PLL)
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

    // Configure the UDMA UART. This UART's settings will be used as the initial console UART.
    // This is configured in the loader so that the log crate does not have a dependency
    // on the cramium-hal crate to be functional.

    // Set up the IO mux to map UART_A0:
    //  UART_RX_A[0] = PA3
    //  UART_TX_A[0] = PA4
    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    iox.set_alternate_function(IoxPort::PA, 3, IoxFunction::AF1);
    iox.set_alternate_function(IoxPort::PA, 4, IoxFunction::AF1);
    // rx as input, with pull-up
    iox.set_gpio_dir(IoxPort::PA, 3, IoxDir::Input);
    iox.set_gpio_pullup(IoxPort::PA, 3, IoxEnable::Enable);
    // tx as output
    iox.set_gpio_dir(IoxPort::PA, 4, IoxDir::Output);

    // Set up the UDMA_UART block to the correct baud rate and enable status
    let mut udma_global = udma::GlobalConfig::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udma_global.clock_on(udma::PeriphId::Uart0);
    udma_global.map_event(
        udma::PeriphId::Uart0,
        udma::PeriphEventType::Uart(udma::EventUartOffset::Rx),
        udma::EventChannel::Channel0,
    );
    udma_global.map_event(
        udma::PeriphId::Uart0,
        udma::PeriphEventType::Uart(udma::EventUartOffset::Tx),
        udma::EventChannel::Channel1,
    );

    let baudrate: u32 = 115200;
    let freq: u32 = 100_000_000;

    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::new(utra::udma_uart_0::HW_UDMA_UART_0_BASE, baudrate, freq)
    };
    let tx_buf = unsafe {
        // safety: it's safe only because we are manually tracking the allocations in IFRAM0. Yuck!
        core::slice::from_raw_parts_mut(
            (utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096) as *mut u8,
            4096,
        )
    };
    // Board bring-up: send characters to confirm the UART is configured & ready to go for the logging crate!
    // The "boot gutter" also has a role to pause the system in "real mode" before VM is mapped in Xous
    // makes things a little bit cleaner for JTAG ops, it seems.
    #[cfg(feature = "board-bringup")]
    {
        let rx_buf = unsafe {
            // safety: it's safe only because we are manually tracking the allocations in IFRAM0. Yuck!
            core::slice::from_raw_parts_mut(
                (utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 8192) as *mut u8,
                1,
            )
        };
        const BANNER: &'static str = "\n\rKeep pressing keys to continue boot...\r\n";
        tx_buf[..BANNER.len()].copy_from_slice(BANNER.as_bytes());
        udma_uart.write(&tx_buf[..BANNER.len()]);

        // receive characters -- print them back. just to prove that this works. no other reason than that.
        for _ in 0..4 {
            udma_uart.read(rx_buf);
            const DBG_MSG: &'static str = "Got: ";
            tx_buf[..DBG_MSG.len()].copy_from_slice(DBG_MSG.as_bytes());
            udma_uart.write(&tx_buf[..DBG_MSG.len()]);
            tx_buf[0] = rx_buf[0];
            udma_uart.write(&tx_buf[..1]);
            tx_buf[0] = '\n' as u32 as u8;
            tx_buf[1] = '\r' as u32 as u8;
            udma_uart.write(&tx_buf[..2]);
        }
    }

    const ONWARD: &'static str = "\n\rBooting!\n\r";
    tx_buf[..ONWARD.len()].copy_from_slice(ONWARD.as_bytes());
    udma_uart.write(&tx_buf[..ONWARD.len()]);
}

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
