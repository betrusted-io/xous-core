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
    let freq: u32 = 45_882_000;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
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

pub unsafe fn init_clock_asic(freq_hz: u32) {
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    const F_MHZ: u32 = 1_000_000;
    const FREQ_0: u32 = 16 * F_MHZ;

    const TBL_Q: [u16; 7] = [
        // keep later DIV even number as possible
        0x7777, // 16-32 MHz
        0x7737, // 32-64
        0x3733, // 64-128
        0x3313, // 128-256
        0x3311, // 256-512 // keep ~ 100MHz
        0x3301, // 512-1024
        0x3301, /* 1024-1500
                 * 0x1303, // 256-512
                 * 0x0103, // 512-1024
                 * 0x0001, // 1024-2048 */
    ];
    const TBL_MUL: [u32; 7] = [
        64, // 16-32 MHz
        32, // 32-64
        16, // 64-128
        8,  // 128-256
        4,  // 256-512
        2,  // 512-1024
        2,  // 1024-2048
    ];
    const M: u32 = 24 - 1;

    report_api(0xc0c0_0000);
    let f16_mhz_log2 = (freq_hz / FREQ_0).ilog2() as usize;
    report_api(f16_mhz_log2 as u32);
    let n_fxp24: u64 = (((freq_hz as u64) << 24) * TBL_MUL[f16_mhz_log2] as u64) / (2 * F_MHZ as u64);
    report_api(n_fxp24 as u32);
    report_api((n_fxp24 >> 32) as u32);
    let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;
    report_api(n_frac);
    let pllmn = ((M << 12) & 0x0001F000) | ((n_fxp24 >> 24) & 0x00000fff) as u32;
    report_api(pllmn);
    let pllf = n_frac | (if 0 == n_frac { 0 } else { 1 << 24 });
    report_api(pllf);
    let pllq = TBL_Q[f16_mhz_log2] as u32;
    report_api(pllq);

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
        for _ in 0..4 {
            report_api(0xc0c0_dddd);
        }

        // printf ("%s(%4" PRIu32 "MHz) M = 24, N = %4lu.%08lu, Q = %2lu\n",
        //     __FUNCTION__, freqHz / 1000000, (uint32_t)(n_fxp24 >>
        // 24).write_volatile((uint32_t)((uint64_t)(n_fxp24 & 0x00ffffff) * 100000000/(1UL
        // <<24)).write_volatile(TBL_MUL[f16MHzLog2]);
        daric_cgu.add(sysctrl::SFR_IPCPLLMN.offset()).write_volatile(pllmn); // 0x1F598; // ??
        daric_cgu.add(sysctrl::SFR_IPCPLLF.offset()).write_volatile(pllf); // ??
        daric_cgu.add(sysctrl::SFR_IPCPLLQ.offset()).write_volatile(pllq); // ?? TODO select DIV for VCO freq

        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        // DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        daric_cgu.add(sysctrl::SFR_IPCCR.offset()).write_volatile((1 << 6) | (2 << 3) | (3));
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32

        daric_cgu
            .add(sysctrl::SFR_IPCLPEN.offset())
            .write_volatile(daric_cgu.add(sysctrl::SFR_IPCLPEN.offset()).read_volatile() & !0x02);
        daric_cgu.add(sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x32); // commit, must write 32

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        for _ in 0..4 {
            report_api(0xc0c0_eeee);
        }
        // printf("read reg a0 : %08" PRIx32"\n", *((volatile uint32_t* )0x400400a0));
        // printf("read reg a4 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a4));
        // printf("read reg a8 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a8));

        // TODO wait/poll lock status?
        daric_cgu.add(sysctrl::SFR_CGUSEL0.offset()).write_volatile(1); // clktop sel, 0:clksys, 1:clkpll0
        daric_cgu.add(sysctrl::SFR_CGUSET.offset()).write_volatile(0x32); // commit

        report_api(0xc0c0_ffff);
        // printf ("    MN: 0x%05x, F: 0x%06x, Q: 0x%04x\n",
        //     DARIC_IPC->pll_mn, DARIC_IPC->pll_f, DARIC_IPC->pll_q);
        // printf ("    LPEN: 0x%01x, OSC: 0x%04x, BIAS: 0x%04x,\n",
        //     DARIC_IPC->lpen, DARIC_IPC->osc, DARIC_IPC->ipc);
    }
    report_api(0xc0c0_0007);
}
