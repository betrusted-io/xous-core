use cramium_hal::iox::{Iox, IoxDir, IoxEnable, IoxFunction, IoxPort};
use cramium_hal::udma;
use utralib::generated::*;

// Notes about the reset vector location
// This can be set using fuses in the IFR (also called 'info') region
// The offset is an 8-bit value, which is shifted into a final location
// according to the following formula:
//
// let short_offset: u8 = OFFSET;
// let phys_offset: u32 = 0x6000_0000 + short_offset << 14;
//
// The RV32-IV IFR fuse location is at row 6, byte 8.
// Each row is 256 bits wide.
// This puts the byte-address hex offset at (6 * 256 + 8 * 8) / 8 = 0xC8
// within the IFR region. Total IFR region size is 0x200.

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;

// Locate the hard-wired IFRAM allocations for UDMA
pub const UART_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4096;
// RAM needs two buffers of 1k + 16 bytes = 2048 + 16 = 2064 bytes; round up to one page
pub const SPIM_RAM_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 2 * 4096;
// Flash will be released after the loader is done: it's only accessed to copy the IniS sectors into swap,
// then abandoned. It needs 4096 bytes for Rx, and 0 bytes for Tx + 16 bytes for cmd.
pub const SPIM_FLASH_IFRAM_ADDR: usize = utralib::HW_IFRAM0_MEM + utralib::HW_IFRAM0_MEM_LEN - 4 * 4096;

// location of kernel, as offset from the base of ReRAM. This needs to match up with what is in link.x.
// inclusive of the signature block offset
pub const KERNEL_OFFSET: usize = 0x4_1000;

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
    use utralib::utra::sysctrl;
    unsafe {
        // this is MANDATORY for any chip stapbility in real silicon, as the initial
        // clocks are too unstable to do anything otherwise. However, for the simulation
        // environment, this can (should?) be dropped
        let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
        daric_cgu.add(sysctrl::SFR_CGUSEL1.offset()).write_volatile(1); // 0: RC, 1: XTAL
        daric_cgu.add(sysctrl::SFR_CGUFSCR.offset()).write_volatile(48); // external crystal is 48MHz

        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(24);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }
    // this block is mandatory in all cases to get clocks set into some consistent, expected mode
    unsafe {
        let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7f7f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x7f7f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x3f3f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x1f1f);
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0f0f);
        daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xFF);
        daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xFF);
        daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xFF);
        daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xFF);
        daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        // enable DUART
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }
    // unsafe, direct-writes to address offsets are used here instead of the UTRA abstraction
    // because there are some quirks in the early boot path that make the system more stable
    // if all register accesses are in-lined.
    #[cfg(feature = "boot-delay")]
    unsafe {
        // this block should immediately follow the CGU setup
        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        // ~2 second delay for debugger to attach
        let msg = b"boot\r";
        for j in 0..20_000 {
            // variable count of .'s to create a sense of motion on the console
            for _ in 0..j & 0x7 {
                while duart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
                duart.add(utra::duart::SFR_TXD.offset()).write_volatile('.' as char as u32);
            }
            for &b in msg {
                while duart.add(utra::duart::SFR_SR.offset()).read_volatile() != 0 {}
                duart.add(utra::duart::SFR_TXD.offset()).write_volatile(b as char as u32);
            }
        }
    }
    #[cfg(feature = "sram-margin")]
    unsafe {
        // set SRAM delay to max - opens up timing margin as much a possible, supposedly?
        let sram_ctl = utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32;
        let waitcycles = 3;
        sram_ctl.add(utra::coresub_sramtrm::SFR_SRAM0.offset()).write_volatile(
            (sram_ctl.add(utra::coresub_sramtrm::SFR_SRAM0.offset()).read_volatile() & !0x18)
                | ((waitcycles << 3) & 0x18),
        );
        sram_ctl.add(utra::coresub_sramtrm::SFR_SRAM1.offset()).write_volatile(
            (sram_ctl.add(utra::coresub_sramtrm::SFR_SRAM1.offset()).read_volatile() & !0x18)
                | ((waitcycles << 3) & 0x18),
        );
    }
    // SoC emulator board parameters (deals with MMCM instead of PLL)
    // Remove this once we feel confident we're sticking with SoC hardware.
    /*
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
    } */

    // Now setup the clocks for real
    // Safety: this can only be called in the early_init boot context
    let perclk = unsafe { init_clock_asic(800_000_000) };
    crate::println!("Perclk is {} Hz", perclk);

    // Configure the UDMA UART. This UART's settings will be used as the initial console UART.
    // This is configured in the loader so that the log crate does not have a dependency
    // on the cramium-hal crate to be functional.

    // Set up the IO mux to map UART_A0:
    //  UART_RX_A[0] = PA3
    //  UART_TX_A[0] = PA4
    //  UART_RX_A[1] = PD13
    //  UART_RX_A[1] = PD14
    let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    iox.set_alternate_function(IoxPort::PD, 13, IoxFunction::AF1);
    iox.set_alternate_function(IoxPort::PD, 14, IoxFunction::AF1);
    // rx as input, with pull-up
    iox.set_gpio_dir(IoxPort::PD, 13, IoxDir::Input);
    iox.set_gpio_pullup(IoxPort::PD, 13, IoxEnable::Enable);
    // tx as output
    iox.set_gpio_dir(IoxPort::PD, 14, IoxDir::Output);

    // Set up the UDMA_UART block to the correct baud rate and enable status
    let mut udma_global = udma::GlobalConfig::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udma_global.clock_on(udma::PeriphId::Uart1);
    udma_global.map_event(
        udma::PeriphId::Uart1,
        udma::PeriphEventType::Uart(udma::EventUartOffset::Rx),
        udma::EventChannel::Channel0,
    );
    udma_global.map_event(
        udma::PeriphId::Uart1,
        udma::PeriphEventType::Uart(udma::EventUartOffset::Tx),
        udma::EventChannel::Channel1,
    );

    let baudrate: u32 = 115200;
    let freq: u32 = perclk / 2;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    crate::println!("Baud freq is {} Hz, baudrate is {}", freq, baudrate);
    udma_uart.set_baud(baudrate, freq);

    // Board bring-up: send characters to confirm the UART is configured & ready to go for the logging crate!
    // The "boot gutter" also has a role to pause the system in "real mode" before VM is mapped in Xous
    // makes things a little bit cleaner for JTAG ops, it seems.
    #[cfg(feature = "board-bringup")]
    {
        // do a quick TRNG test.
        let mut trng = cramium_hal::sce::trng::Trng::new(HW_TRNG_BASE);
        trng.setup_raw_generation(256);
        for _ in 0..8 {
            crate::println!("trng raw: {:x}", trng.get_u32().unwrap_or(0xDEAD_BEEF));
        }
        let trng_csr = CSR::new(HW_TRNG_BASE as *mut u32);
        crate::println!("trng status: {:x}", trng_csr.r(utra::trng::SFR_SR));

        // do a PL230/PIO test. Toggles PB15 (PIO0) with an LFSR sequence.
        let mut pl230 = xous_pl230::Pl230::new();
        xous_pl230::pl230_tests::units::basic_tests(&mut pl230);
        // xous_pl230::pl230_tests::units::pio_test(&mut pl230);

        const BANNER: &'static str = "\n\rKeep pressing keys to continue boot...\r\n";
        udma_uart.write(BANNER.as_bytes());

        // space for one character, plus appending CRLF for the return
        let mut rx_buf = [0u8; 3];

        // receive characters -- print them back. just to prove that this works. no other reason than that.
        for _ in 0..4 {
            udma_uart.read(&mut rx_buf[..1]);
            const DBG_MSG: &'static str = "Got: ";
            udma_uart.write(&DBG_MSG.as_bytes());
            rx_buf[1] = '\n' as u32 as u8;
            rx_buf[2] = '\r' as u32 as u8;
            udma_uart.write(&rx_buf);
        }
    }

    const ONWARD: &'static str = "\n\rBooting!\n\r";
    udma_uart.write(&ONWARD.as_bytes());
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

// returns the actual per_clk
unsafe fn init_clock_asic(freq_hz: u32) -> u32 {
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    /*
       Code notes from RTL:
       assign pll_m = ipc_pllmn[16:12];
       assign pll_n = ipc_pllmn[11: 0];
       assign pll_f = ipc_pllf[23: 0];
       assign pll_fen = ipc_pllf[24];
       assign pll_q00 = ipc_pllq[ 2: 0];
       assign pll_q10 = ipc_pllq[ 6: 4];
       assign pll_q01 = ipc_pllq[10: 8];
       assign pll_q11 = ipc_pllq[14:12];

       Clko0 = Fvco / (pllq[ 2:0] + 1) / (pllq[ 6:4] + 1)
       Clko1 = Fvco / (pllq[10:8] + 1) / (pllq[14:12] + 1)
       Fvco target is 2GHz (1-3GHz range)

      .gvco_bias ( pll_bias[7:6] ),
      .cpp_bias  ( pll_bias[5:3] ),
      .cpi_bias  ( pll_bias[2:0] ),
    */
    // Derive VCO frequency from legal, even dividers that get us close to our target frequency
    const TARGET_VCO_HZ: u32 = 1_600_000_000; // 1.6GHz
    let final_div: u32 = TARGET_VCO_HZ / freq_hz;
    // fclk_div has to be a power of 2
    let fclk_div =
        if (1 << final_div.ilog2()) != final_div { 1 << (final_div.ilog2() + 1) } else { final_div };
    let vco_actual: u32 = fclk_div * freq_hz;
    if vco_actual < 1_000_000_000 || vco_actual > 3_000_000_000 {
        crate::println!("Warning: VCO out of range: {}", vco_actual);
    }
    const TARGET_PERCLK_HZ: u32 = 100_000_000; // 100 MHz
    let perclk_np_div: u32 = vco_actual / TARGET_PERCLK_HZ;
    let perclk_div = if (1 << perclk_np_div.ilog2()) != perclk_np_div {
        1 << (perclk_np_div.ilog2() + 1)
    } else {
        perclk_np_div
    };
    let ilog2_fdiv = fclk_div.ilog2();
    let ilog2_pdiv = perclk_div.ilog2();
    let pll_q0_0 = (1 << (ilog2_fdiv / 2)) - 1;
    let pll_q1_0 = (1 << (ilog2_fdiv / 2 + ilog2_fdiv % 2)) - 1;
    let pll_q0_1 = (1 << (ilog2_pdiv / 2)) - 1;
    let pll_q1_1 = (1 << (ilog2_pdiv / 2 + ilog2_pdiv % 2)) - 1;
    if pll_q0_0 > 7 || pll_q0_1 > 7 || pll_q1_0 > 7 || pll_q1_1 > 7 {
        crate::println!(
            "Warning: PLLQ out of range: 0_0:{} 1_0:{} 0_1:{} 1_1:{}",
            pll_q0_0,
            pll_q1_0,
            pll_q0_1,
            pll_q1_1
        );
    }
    // this is the pllq value
    let pllq = (pll_q0_0 & 7) | ((pll_q1_0 & 7) << 4) | ((pll_q0_1 & 7) << 8) | ((pll_q1_1 & 7) << 12);

    // now, program the VCO to get to as close to vco_actual
    const FREF_HZ: u32 = 48_000_000;
    let ni = vco_actual / FREF_HZ;
    if ni >= 4096 || ni < 8 {
        crate::println!("Warning: ni out of range: {}", ni);
    }
    let pllmn = (1 << 12) | ni & 0xFFF; // m is set to 1, lower 12 bits is nf
    let frac_n = ((vco_actual as f32 / FREF_HZ as f32) - ni as f32).max(0 as f32);
    let pllf: u32 = (frac_n * ((1 << 24) as f32)) as u32;
    if pllf >= 1 << 24 {
        crate::println!("Warning nf out of range: 0x{:x}", pllf);
    }
    let n_frac = if pllf != 0 { pllf | 1 << 24 } else { 0 }; // set the frac enable bit if needed

    crate::println!("pllq: 0x{:x}, pllmn: 0x{:x}, n_frac: 0x{:x}", pllq, pllmn, n_frac);

    daric_cgu.add(sysctrl::SFR_CGUSEL1.offset()).write_volatile(1); // 0: RC, 1: XTAL
    daric_cgu.add(sysctrl::SFR_CGUFSCR.offset()).write_volatile(48); // external crystal is 48MHz
    daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

    if freq_hz < 1_000_000 {
        daric_cgu.add(sysctrl::SFR_IPCOSC.offset()).write_volatile(freq_hz);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32
    }
    // switch to OSC
    daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(0); // clktop sel, 0:clksys, 1:clkpll0
    daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit

    if 0 == freq_hz {
        // do nothing
    } else {
        // PD PLL
        daric_cgu
            .add(sysctrl::SFR_IPCLPEN.offset())
            .write_volatile(daric_cgu.add(sysctrl::SFR_IPCLPEN.offset()).read_volatile() | 0x02);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 1");

        daric_cgu.add(sysctrl::SFR_IPCPLLMN.offset()).write_volatile(pllmn); // 0x1F598;
        daric_cgu.add(sysctrl::SFR_IPCPLLF.offset()).write_volatile(n_frac); // 0x2812
        daric_cgu.add(sysctrl::SFR_IPCPLLQ.offset()).write_volatile(pllq); // 0x2401 TODO select DIV for VCO freq

        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        // DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        daric_cgu.add(sysctrl::SFR_IPCCR.offset()).write_volatile((1 << 6) | (2 << 3) | (3));
        // daric_cgu.add(sysctrl::SFR_IPCCR.offset()).write_volatile((3 << 6) | (5 << 3) | (5));
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32

        daric_cgu
            .add(sysctrl::SFR_IPCLPEN.offset())
            .write_volatile(daric_cgu.add(sysctrl::SFR_IPCLPEN.offset()).read_volatile() & !0x02);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 2");

        daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(1); // clktop sel, 0:clksys, 1:clkpll0
        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit

        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 3");

        crate::println!("fsvalid: {}", daric_cgu.add(sysctrl::SFR_CGUFSVLD.offset()).read_volatile());
        let cgufsfreq0 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ0.offset()).read_volatile();
        let cgufsfreq1 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ1.offset()).read_volatile();
        let cgufsfreq2 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ2.offset()).read_volatile();
        let cgufsfreq3 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ3.offset()).read_volatile();
        crate::println!(
            "Internal osc: {} -> {} MHz ({} MHz)",
            cgufsfreq0,
            fsfreq_to_hz(cgufsfreq0),
            fsfreq_to_hz_32(cgufsfreq0)
        );
        crate::println!(
            "XTAL: {} -> {} MHz ({} MHz)",
            cgufsfreq1,
            fsfreq_to_hz(cgufsfreq1),
            fsfreq_to_hz_32(cgufsfreq1)
        );
        crate::println!(
            "pll output 0: {} -> {} MHz ({} MHz)",
            cgufsfreq2,
            fsfreq_to_hz(cgufsfreq2),
            fsfreq_to_hz_32(cgufsfreq2)
        );
        crate::println!(
            "pll output 1: {} -> {} MHz ({} MHz)",
            cgufsfreq3,
            fsfreq_to_hz(cgufsfreq3),
            fsfreq_to_hz_32(cgufsfreq3)
        );

        // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
        // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7fff); // fclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk
    }
    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);

    vco_actual / perclk_div
}

fn fsfreq_to_hz(fs_freq: u32) -> u32 { (fs_freq * (48_000_000 / 32)) / 1_000_000 }
fn fsfreq_to_hz_32(fs_freq: u32) -> u32 { (fs_freq * (32_000_000 / 32)) / 1_000_000 }
