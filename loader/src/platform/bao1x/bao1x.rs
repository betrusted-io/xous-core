#[allow(unused_imports)]
use core::convert::TryInto;

use bao1x_api::signatures::SIGBLOCK_LEN;
#[allow(unused_imports)]
use bao1x_api::*;
#[allow(unused_imports)]
use bao1x_hal::iox::Iox;
#[cfg(feature = "bao1x")]
use bao1x_hal::udma;
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

// Define the .data region - bootstrap baremetal using these hard-coded parameters.
pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;

// location of kernel, as offset from the base of RRAM.
pub const KERNEL_OFFSET: usize = bao1x_api::offsets::KERNEL_START - utralib::generated::HW_RERAM_MEM;

#[allow(dead_code)]
pub fn delay(quantum: usize) {
    use utralib::{CSR, utra};
    // abuse the d11ctime timer to create some time-out like thing
    let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
    d11c.wfo(utra::d11ctime::CONTROL_COUNT, 333_333); // 1.0ms per interval
    let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    for _ in 0..quantum {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
        polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
    }
    // we have to split this because we don't know where we caught the previous interval
    if quantum == 1 {
        while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
    }
}

pub fn early_init() -> u32 {
    // For the loader, the statics structure is located just after the signature block
    const STATICS_LOC: usize = bao1x_api::LOADER_START + SIGBLOCK_LEN;

    // safety: this data structure is pre-loaded by the image loader and is guaranteed to
    // only have representable, valid values that are aligned according to the repr(C) spec
    let statics_in_rom: &bao1x_api::StaticsInRom =
        unsafe { (STATICS_LOC as *const bao1x_api::StaticsInRom).as_ref().unwrap() };
    assert!(statics_in_rom.version == bao1x_api::STATICS_IN_ROM_VERSION, "Can't find valid statics table");

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    // Safety: only safe if the values computed by the loader are correct.
    // Question: this happens before we setup any clocks, timing, etc. I think the CPU is running
    // in a "slow enough" state that these writes should happen, but this may need to be re-ordered
    // in particular with respect to SRAM trimming if there are boot issues discovered in the field.
    unsafe {
        let data_ptr = statics_in_rom.data_origin as *mut u32;
        for i in 0..statics_in_rom.data_size_bytes as usize / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for &(offset, data) in &statics_in_rom.poke_table[..statics_in_rom.valid_pokes as usize] {
            data_ptr
                .add(u16::from_le_bytes(offset) as usize / size_of::<u32>())
                .write_volatile(u32::from_le_bytes(data));
        }
    }

    #[cfg(not(feature = "verilator-only"))]
    let ret = early_init_hw();

    // return a fake clock result
    #[cfg(feature = "verilator-only")]
    let ret = 100_000_000;

    ret
}

#[cfg(all(feature = "bao1x", not(feature = "verilator-only")))]
pub fn early_init_hw() -> u32 {
    // TODO: we might want to not clear this in the loader so the OS can read the wakeup reason?
    let mut ao_sysctrl = CSR::new(utra::ao_sysctrl::HW_AO_SYSCTRL_BASE as *mut u32);
    // clear any AO wakeup pending bits
    let fr = ao_sysctrl.r(utra::ao_sysctrl::SFR_AOFR);
    ao_sysctrl.wo(utra::ao_sysctrl::SFR_AOFR, fr);

    // ASSUME:
    //   - clocks and SRAM timings are set up by boot1, and perclk is at 100MHz
    //   - UART2 is up and running as console
    let perclk = 100_000_000;

    #[cfg(feature = "board-baosec")]
    {
        // if board type is the default (dabao), reset to baosec, and reboot so that key
        // provisioning can run. This should only happen if the avalanche generator is known to be good.
        // TODO: implement avalanche generator test?
        let one_way = bao1x_hal::acram::OneWayCounter::new();
        let board_type =
            one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("Board type coding error");
        if board_type != bao1x_api::BoardTypeCoding::Baosec {
            crate::println!("Board type is not Baosec; resetting it and rebooting!");
            while one_way.get_decoded::<bao1x_api::BoardTypeCoding>().expect("owc coding error")
                != bao1x_api::BoardTypeCoding::Baosec
            {
                one_way.inc_coded::<bao1x_api::BoardTypeCoding>().expect("increment error");
            }
            crate::println!("Board type set to baosec, rebooting so boot1 can provision keys!");
            let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
            rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);
        }

        // setup all the board pins to a known state
        let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
        bao1x_hal::board::setup_display_pins(&iox);
        bao1x_hal::board::setup_memory_pins(&iox);
        bao1x_hal::board::setup_i2c_pins(&iox);
        bao1x_hal::board::setup_camera_pins(&iox);
        bao1x_hal::board::setup_kb_pins(&iox);
        bao1x_hal::board::setup_oled_power_pin(&iox);
        let trng_power = bao1x_hal::board::setup_trng_power_pin(&iox);
        // kernel expects the TRNG to be on
        iox.set_gpio_pin(trng_power.0, trng_power.1, bao1x_api::IoxValue::High);

        use bao1x_hal::udma::GlobalConfig;
        use ux_api::minigfx::FrameBuffer;

        let mut iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
        // redraw or show the boot logo - depending on how boot1 went, we may or may not have it already
        let mut udma_global = GlobalConfig::new();
        let mut sh1107 = bao1x_hal::sh1107::Oled128x128::new(
            bao1x_hal::sh1107::MainThreadToken::new(),
            perclk,
            &mut iox,
            &mut udma_global,
        );
        sh1107.init();
        sh1107.buffer_mut().fill(0xFFFF_FFFF);
        sh1107.blit_screen(&ux_api::bitmaps::baochip128x128::BITMAP);
        sh1107.draw();
    }

    #[cfg(feature = "board-dabao")]
    {
        let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

        // this needs to be set here because alpha-0 boot1 images set it to the wrong frequency
        let perclk = unsafe { init_clock_asic(bao1x_api::dabao::DEFAULT_FCLK_FREQUENCY) };
        // fixup the UART baud rate
        let _udma_uart = setup_console(&bao1x_api::BoardTypeCoding::Dabao, &iox, perclk);
        bao1x_hal::board::setup_console_pins(&iox);

        // set bootwait exactly once - this makes the default behavior to bootwait,
        // but allows devs to override it by incrementing the value
        let one_way = bao1x_hal::acram::OneWayCounter::new();
        if one_way.get(bao1x_api::BootWaitCoding::OFFSET).unwrap() == 0 {
            one_way.inc_coded::<bao1x_api::BootWaitCoding>().ok();
        }
    }

    // Setup some global control registers that will allow the TRNG to operate once the kernel is
    // booted. This is done so the kernel doesn't have to exclusively have rights to the SCE global
    // registers just for this purpose.
    let mut glbl_csr = CSR::new(utralib::utra::sce_glbsfr::HW_SCE_GLBSFR_BASE as *mut u32);
    glbl_csr.wo(utra::sce_glbsfr::SFR_SUBEN, 0xff);
    glbl_csr.wo(utra::sce_glbsfr::SFR_FFEN, 0x30);
    glbl_csr.wo(utra::sce_glbsfr::SFR_FFCLR, 0xff05);

    // this should go to the serial console, because boot1 setup the console for us
    crate::println!("\n\r~~ Xous Loader ~~\n\r");

    perclk
}

#[cfg(feature = "board-dabao")]
pub fn setup_console<T: IoSetup + IoGpio>(
    board_type: &bao1x_api::BoardTypeCoding,
    iox: &T,
    perclk: u32,
) -> bao1x_hal::udma::Uart {
    use bao1x_hal::udma::{GlobalConfig, UartIrq};

    let uart_id = match board_type {
        BoardTypeCoding::Baosec => bao1x_hal::board::setup_console_pins(iox),
        BoardTypeCoding::Dabao | BoardTypeCoding::Oem => {
            // note: we can borrow the baosec console setup only because they
            // happen to map to the same pins. OEM variants that choose different
            // pins will need to add their own case here.
            bao1x_hal::board::setup_console_pins(iox)
        }
    };
    let udma_global = GlobalConfig::new();

    udma_global.clock_on(uart_id);
    udma_global.map_event(uart_id, PeriphEventType::Uart(EventUartOffset::Rx), EventChannel::Channel0);
    udma_global.map_event(uart_id, PeriphEventType::Uart(EventUartOffset::Tx), EventChannel::Channel1);

    let baudrate: u32 = bao1x_api::UART_BAUD;
    let freq: u32 = perclk / 2;

    // the address of the UART buffer is "hard-allocated" at an offset one page from the top of
    // IFRAM0. This is a convention that must be respected by the UDMA UART library implementation
    // for things to work.
    let uart_buf_addr = crate::UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    udma_uart.set_baud(baudrate, freq);
    udma_uart.setup_async_read();

    // setup interrupt here
    let mut uart_irq = UartIrq::new();
    uart_irq.rx_irq_ena(uart_id.try_into().expect("couldn't convert uart_id"), true);

    udma_uart
}

// returns the actual per_clk
#[cfg(not(feature = "simulation-only"))]
#[allow(dead_code)]
pub unsafe fn init_clock_asic(freq_hz: u32) -> u32 {
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    const UNIT_MHZ: u32 = 1000 * 1000;
    const PFD_F_MHZ: u32 = 16;
    const FREQ_0: u32 = 16 * UNIT_MHZ;
    const FREQ_OSC_MHZ: u32 = 48; // Actually 48MHz
    const M: u32 = FREQ_OSC_MHZ / PFD_F_MHZ; //  - 1;  // OSC input was 24, replace with 48

    const TBL_Q: [u16; 7] = [
        // keep later DIV even number as possible
        0x7777, // 16-32 MHz
        0x7737, // 32-64
        0x3733, // 64-128
        0x3313, // 128-256
        0x3311, // 256-512 // keep ~ 100MHz
        0x3301, // 512-1024
        0x3301, // 1024-1600
    ];
    const TBL_MUL: [u32; 7] = [
        64, // 16-32 MHz
        32, // 32-64
        16, // 64-128
        8,  // 128-256
        4,  // 256-512
        2,  // 512-1024
        2,  // 1024-1600
    ];

    // Safest divider settings, assuming no overclocking.
    // If overclocking, need to lower hclk:iclk:pclk even futher; the CPU speed can outperform the bus fabric.
    // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
    // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f7f); // fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk

    // calculate perclk divider. Target 100MHz.
    let (min_cycle, fd, perclk) = if let Some((min_cycle, fd, perclk)) = clk_to_per(freq_hz / 1_000_000, 100)
    {
        daric_cgu
            .add(utra::sysctrl::SFR_CGUFDPER.offset())
            .write_volatile((min_cycle as u32) << 16 | (fd as u32) << 8 | fd as u32);
        (min_cycle, fd, perclk * 1_000_000)
    } else if freq_hz > 400_000_000 {
        daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x07_ff_ff);
        (7, 0xff, freq_hz / 8)
    } else {
        daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
        (3, 0xff, freq_hz / 4)
    };

    /*
        perclk fields:  min-cycle-lp | min-cycle | fd-lp | fd
        clkper fd
            0xff :   Fperclk = Fclktop/2
            0x7f:   Fperclk = Fclktop/4
            0x3f :   Fperclk = Fclktop/8
            0x1f :   Fperclk = Fclktop/16
            0x0f :   Fperclk = Fclktop/32
            0x07 :   Fperclk = Fclktop/64
            0x03:   Fperclk = Fclktop/128
            0x01:   Fperclk = Fclktop/256

        min cycle of clktop, F means frequency
        Fperclk  Max = Fperclk/(min cycle+1)*2
    */

    // turn off gates
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xff);
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

    // set voltage regulators to 0.893v. This is necessary because lp mode may set it lower.
    if freq_hz > 700_000_000 {
        crate::println!("setting vdd85 to 0.893v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421FF1);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    } else if freq_hz > 350_000_000 {
        crate::println!("setting vdd85 to 0.81v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421290);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    } else {
        crate::println!("setting vdd85 to 0.72v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420420);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    }
    crate::println!("...done");

    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    if freq_hz <= 1_000_000 {
        cgu.wo(sysctrl::SFR_IPCOSC, freq_hz);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
    }
    // switch to OSC
    // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    // commit
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    if freq_hz <= 1_000_000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        for _ in 0..100 {
            crate::print!("4");
        }
        crate::println!(".");

        // PD PLL
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
        // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 1");
        // why is this print needed for the code not to crash?
        crate::println!("freq_hz {} log2 {}", freq_hz, f16mhz_log2);
        n_fxp24 = (((freq_hz as u64) << 24) * TBL_MUL[f16mhz_log2 as usize] as u64
            + PFD_F_MHZ as u64 * UNIT_MHZ as u64 / 2)
            / (PFD_F_MHZ as u64 * UNIT_MHZ as u64); // rounded
        let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;

        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
        // DARIC_IPC->pll_f = n_frac | ((0 == n_frac) ? 0 : (1UL << 24));
        cgu.wo(sysctrl::SFR_IPCPLLF, n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
        // DARIC_IPC->pll_q = TBL_Q[f16MHzLog2]; // ?? TODO select DIV for VCO freq
        cgu.wo(sysctrl::SFR_IPCPLLQ, TBL_Q[f16mhz_log2 as usize] as u32);
        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        //DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        // DARIC_IPC->ipc = (1 << 6) | (2 << 3) | (3);
        cgu.wo(sysctrl::SFR_IPCCR, (1 << 6) | (2 << 3) | (3));
        // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);

        // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 2");

        // TODO wait/poll lock status?
        // DARIC_CGU->cgusel0 = 1; // clktop sel, 0:clksys, 1:clkpll0
        cgu.wo(sysctrl::SFR_CGUSEL0, 1);
        // __DSB();
        // DARIC_CGU->cguset = 0x32; // commit
        cgu.wo(sysctrl::SFR_CGUSET, 0x32);
        crate::println!("clocks set");
    }
    crate::println!(
        "mn {:x}, q{:x}",
        (0x400400a0 as *const u32).read_volatile(),
        (0x400400a8 as *const u32).read_volatile()
    );

    crate::println!("fsvalid: {}", daric_cgu.add(sysctrl::SFR_CGUFSVLD.offset()).read_volatile());
    let clk_desc: [(&'static str, u32, usize); 8] = [
        ("fclk", 16, 0x40 / size_of::<u32>()),
        ("pke", 0, 0x40 / size_of::<u32>()),
        ("ao", 16, 0x44 / size_of::<u32>()),
        ("aoram", 0, 0x44 / size_of::<u32>()),
        ("osc", 16, 0x48 / size_of::<u32>()),
        ("xtal", 0, 0x48 / size_of::<u32>()),
        ("pll0", 16, 0x4c / size_of::<u32>()),
        ("pll1", 0, 0x4c / size_of::<u32>()),
    ];
    for (name, shift, offset) in clk_desc {
        let fsfreq = (daric_cgu.add(offset).read_volatile() >> shift) & 0xffff;
        crate::println!("{}: {} MHz", name, fsfreq);
    }
    // Taken in from latest daric_util.c
    let mut udmacore = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udmacore.wo(utra::udma_ctrl::REG_CG, 0xFFFF_FFFF);

    crate::println!("Perclk solution: {:x}|{:x} -> {} MHz", min_cycle, fd, perclk / 1_000_000);
    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);
    perclk
}

#[allow(dead_code)]
fn fsfreq_to_hz(fs_freq: u32) -> u32 { (fs_freq * (48_000_000 / 32)) / 1_000_000 }

#[allow(dead_code)]
fn fsfreq_to_hz_32(fs_freq: u32) -> u32 { (fs_freq * (32_000_000 / 32)) / 1_000_000 }

#[allow(dead_code)]
#[cfg(feature = "bao1x")]
/// Used mainly for debug breaks. Not used in every configuration.
pub fn getc() -> char {
    let uart_buf_addr = loader::UART_IFRAM_ADDR;
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    let mut rx_buf = [0u8; 1];
    udma_uart.read(&mut rx_buf);
    char::from_u32(rx_buf[0] as u32).unwrap_or(' ')
}

#[cfg(all(feature = "verilator-only", not(feature = "bao1x-mpw")))]
pub fn coreuser_config() {
    // configure coruser signals. Specific to bao1x.
    use utra::coreuser::*;
    crate::println!("coreuser setup...");
    let mut coreuser = CSR::new(utra::coreuser::HW_COREUSER_BASE as *mut u32);
    // set to 0 so we can safely mask it later on
    coreuser.wo(USERVALUE, 0);
    coreuser.rmwf(utra::coreuser::USERVALUE_DEFAULT, 3);
    let trusted_asids = [(1, 0), (2, 0), (3, 1), (4, 2), (1, 0), (1, 0), (1, 0), (1, 0)];
    let asid_fields = [
        (utra::coreuser::MAP_LO_LUT0, utra::coreuser::USERVALUE_USER0),
        (utra::coreuser::MAP_LO_LUT1, utra::coreuser::USERVALUE_USER1),
        (utra::coreuser::MAP_LO_LUT2, utra::coreuser::USERVALUE_USER2),
        (utra::coreuser::MAP_LO_LUT3, utra::coreuser::USERVALUE_USER3),
        (utra::coreuser::MAP_HI_LUT4, utra::coreuser::USERVALUE_USER4),
        (utra::coreuser::MAP_HI_LUT5, utra::coreuser::USERVALUE_USER5),
        (utra::coreuser::MAP_HI_LUT6, utra::coreuser::USERVALUE_USER6),
        (utra::coreuser::MAP_HI_LUT7, utra::coreuser::USERVALUE_USER7),
    ];
    for (&(asid, value), (map_field, uservalue_field)) in trusted_asids.iter().zip(asid_fields) {
        coreuser.rmwf(map_field, asid);
        coreuser.rmwf(uservalue_field, value);
    }
    coreuser.rmwf(CONTROL_INVERT_PRIV, 1);
    coreuser.rmwf(CONTROL_ENABLE, 1);

    // turn off updates
    coreuser.wo(utra::coreuser::PROTECT, 1);
    crate::println!("coreuser locked!");
}

/// Takes in the top clock in MHz, desired perclk in MHz, and returns a tuple of
/// (min cycle, fd, actual freq)
/// *tested*
#[allow(dead_code)]
pub fn clk_to_per(top_in_mhz: u32, perclk_in_mhz: u32) -> Option<(u8, u8, u32)> {
    let fd_platonic = ((256 * perclk_in_mhz) / (top_in_mhz / 2)).min(256);
    if fd_platonic > 0 {
        let fd = fd_platonic - 1;
        let min_cycle = (2 * (256 / (fd + 1))).max(1);
        let min_freq = top_in_mhz / min_cycle;
        let target_freq = top_in_mhz * (fd + 1) / 512;
        let actual_freq = target_freq.max(min_freq);
        if fd < 256 && min_cycle < 256 && min_cycle > 0 {
            Some(((min_cycle - 1) as u8, fd as u8, actual_freq))
        } else {
            None
        }
    } else {
        None
    }
}

/// Delay with a given system clock frequency. Useful during power mode switching.
#[allow(dead_code)]
pub fn delay_at_sysfreq(ms: usize, sysclk_freq: u32) {
    let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, sysclk_freq / 1000);
    timer.wfo(utra::timer0::EN_EN, 1);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        // comment this out for testing on MPW
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}
