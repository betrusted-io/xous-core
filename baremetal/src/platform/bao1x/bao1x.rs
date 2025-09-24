use bao1x_api::*;
use bao1x_hal::iox::Iox;
use bao1x_hal::udma;
use utralib::CSR;
use utralib::utra;
use utralib::utra::sysctrl;

use crate::platform::{
    debug::setup_rx,
    irq::{enable_irq, irq_setup},
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;
pub const SIGBLOCK_LEN: usize = 768; // this is adjusted inside builder.rs, in the sign-image invocation

const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

// scratch page for exceptions located at top of RAM
// NOTE: there is an additional page above this for exception stack
pub const SCRATCH_PAGE: usize = HEAP_START - 8192;

pub const UART_IFRAM_ADDR: usize = bao1x_hal::board::UART_DMA_TX_BUF_PHYS;

// the 800_000_000 setting is tested to work at least on one sample
pub const SYSTEM_CLOCK_FREQUENCY: u32 = 800_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

pub fn early_init() {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    #[cfg(not(feature = "bao1x-evb"))]
    {
        // sets up the FET control for DCDC2 (only useful on boards that support it)
        iox.set_gpio_pin_value(IoxPort::PA, 5, IoxValue::High);
        iox.setup_pin(
            IoxPort::PA,
            5,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            Some(IoxEnable::Enable),
            None,
            Some(IoxDriveStrength::Drive2mA),
        );
    }

    let uart = crate::debug::Uart {};
    uart.putc('*' as u32 as u8);

    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    unsafe {
        // this block is mandatory in all cases to get clocks set into some consistent, expected mode
        {
            // conservative dividers
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x3f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x1f3f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0f1f);
            // ungate all clocks
            daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xFF);
            // commit clocks
            daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        // enable DUART
        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        // based on ringosc trimming as measured by oscope. this will get precise after we set the PLL.
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(34);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }
    // sram0 requires 1 wait state for writes
    let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM0, 0x8);
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM1, 0x8);

    #[cfg(feature = "v0p9")]
    {
        /*
        logic [15:0] trm_ram32kx72      ; assign trm_ram32kx72      = trmdat[0 ]; localparam t_trm IV_trm_ram32kx72      = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_ram8kx72       ; assign trm_ram8kx72       = trmdat[1 ]; localparam t_trm IV_trm_ram8kx72       = IV_sram_sp_hde_inst_sram1;
        logic [15:0] trm_rf1kx72        ; assign trm_rf1kx72        = trmdat[2 ]; localparam t_trm IV_trm_rf1kx72        = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf256x27       ; assign trm_rf256x27       = trmdat[3 ]; localparam t_trm IV_trm_rf256x27       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf512x39       ; assign trm_rf512x39       = trmdat[4 ]; localparam t_trm IV_trm_rf512x39       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf128x31       ; assign trm_rf128x31       = trmdat[5 ]; localparam t_trm IV_trm_rf128x31       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_dtcm8kx36      ; assign trm_dtcm8kx36      = trmdat[6 ]; localparam t_trm IV_trm_dtcm8kx36      = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_itcm32kx18     ; assign trm_itcm32kx18     = trmdat[7 ]; localparam t_trm IV_trm_itcm32kx18     = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_ifram32kx36    ; assign trm_ifram32kx36    = trmdat[8 ]; localparam t_trm IV_trm_ifram32kx36    = IV_sram_sp_uhde_inst;
        logic [15:0] trm_sce_sceram_10k ; assign trm_sce_sceram_10k = trmdat[9 ]; localparam t_trm IV_trm_sce_sceram_10k = IV_sram_sp_hde_inst;
        logic [15:0] trm_sce_hashram_3k ; assign trm_sce_hashram_3k = trmdat[10]; localparam t_trm IV_trm_sce_hashram_3k = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aesram_1k  ; assign trm_sce_aesram_1k  = trmdat[11]; localparam t_trm IV_trm_sce_aesram_1k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_pkeram_4k  ; assign trm_sce_pkeram_4k  = trmdat[12]; localparam t_trm IV_trm_sce_pkeram_4k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aluram_3k  ; assign trm_sce_aluram_3k  = trmdat[13]; localparam t_trm IV_trm_sce_aluram_3k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_mimmdpram  ; assign trm_sce_mimmdpram  = trmdat[14]; localparam t_trm IV_trm_sce_mimmdpram  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rdram1kx32     ; assign trm_rdram1kx32     = trmdat[15]; localparam t_trm IV_trm_rdram1kx32     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram512x64    ; assign trm_rdram512x64    = trmdat[16]; localparam t_trm IV_trm_rdram512x64    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram128x22    ; assign trm_rdram128x22    = trmdat[17]; localparam t_trm IV_trm_rdram128x22    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram32x16     ; assign trm_rdram32x16     = trmdat[18]; localparam t_trm IV_trm_rdram32x16     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_bioram1kx32    ; assign trm_bioram1kx32    = trmdat[19]; localparam t_trm IV_trm_bioram1kx32    = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_tx_fifo128x32  ; assign trm_tx_fifo128x32  = trmdat[20]; localparam t_trm IV_trm_tx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rx_fifo128x32  ; assign trm_rx_fifo128x32  = trmdat[21]; localparam t_trm IV_trm_rx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_fifo32x19      ; assign trm_fifo32x19      = trmdat[22]; localparam t_trm IV_trm_fifo32x19      = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_share   ; assign trm_udcmem_share   = trmdat[23]; localparam t_trm IV_trm_udcmem_share   = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_odb     ; assign trm_udcmem_odb     = trmdat[24]; localparam t_trm IV_trm_udcmem_odb     = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_256x64  ; assign trm_udcmem_256x64  = trmdat[25]; localparam t_trm IV_trm_udcmem_256x64  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_acram2kx64     ; assign trm_acram2kx64     = trmdat[26]; localparam t_trm IV_trm_acram2kx64     = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_aoram1kx36     ; assign trm_aoram1kx36     = trmdat[27]; localparam t_trm IV_trm_aoram1kx36     = IV_sram_sp_hde_inst;

             */
        crate::println!("setting 0.9v sramtrm");
        let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
        sramtrm.wo(utra::coresub_sramtrm::SFR_CACHE, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_ITCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_DTCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_VEXRAM, 0x1);

        let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
        // bio 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (19 << 16) | 0b011_000_01_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // vex 0.9v settings
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((15 + i) << 16) | 0b001_010_00_0_0_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }

        // sram 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (0 << 16) | 0b011_000_01_0_1_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (1 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        crate::println!("setting other 0.9v trims");

        // tcm 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (6 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (7 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // ifram 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (8 << 16) | 0b010_000_00_0_1_000_1_01);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // sce 0.9V
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (9 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((10 + i) << 16) | 0b011_000_01_0_1_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (14 << 16) | 0b001_010_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
    }

    // Now that SRAM trims are setup, initialize all the statics by writing to memory.
    // For baremetal, the statics structure is just at the flash base.
    const STATICS_LOC: usize = FLASH_BASE + SIGBLOCK_LEN;

    // safety: this data structure is pre-loaded by the image loader and is guaranteed to
    // only have representable, valid values that are aligned according to the repr(C) spec
    let statics_in_rom: &bao1x_api::StaticsInRom =
        unsafe { (STATICS_LOC as *const bao1x_api::StaticsInRom).as_ref().unwrap() };
    assert!(statics_in_rom.version == bao1x_api::STATICS_IN_ROM_VERSION, "Can't find valid statics table");

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    // Safety: only safe if the values computed by the loader are correct.
    unsafe {
        let data_ptr = statics_in_rom.data_origin as *mut u32;
        for i in 0..statics_in_rom.data_size_bytes as usize / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for &(offset, data) in &statics_in_rom.poke_table[..statics_in_rom.valid_pokes as usize] {
            data_ptr.add(offset as usize).write_volatile(data);
        }
    }

    // set the clock
    let perclk = unsafe { init_clock_asic(SYSTEM_CLOCK_FREQUENCY) };

    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);

    // setup heap alloc
    setup_alloc();

    setup_timer();

    // Rx setup
    let mut udma_uart = setup_rx(perclk);
    irq_setup();
    enable_irq(utra::irqarray5::IRQARRAY5_IRQ);

    udma_uart.write("console up\r\n".as_bytes());
    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);
    crate::println!("This debug print should be on the UDMA UART");

    // Setup I/Os so things that should be powered off are actually off
    bao1x_hal::board::setup_display_pins(&iox);
    bao1x_hal::board::setup_memory_pins(&iox);
    bao1x_hal::board::setup_i2c_pins(&iox);
    bao1x_hal::board::setup_camera_pins(&iox);
    bao1x_hal::board::setup_kb_pins(&iox);
    bao1x_hal::board::setup_oled_power_pin(&iox);
    bao1x_hal::board::setup_trng_power_pin(&iox);

    #[cfg(not(feature = "bao1x-evb"))]
    {
        crate::println!("Engage DCDC2");
        let i2c_channel = bao1x_hal::board::setup_i2c_pins(&iox);
        use bao1x_hal::udma::GlobalConfig;
        let udma_global = GlobalConfig::new();
        udma_global.clock(PeriphId::from(i2c_channel), true);
        let i2c_ifram = unsafe {
            bao1x_hal::ifram::IframRange::from_raw_parts(
                bao1x_hal::board::I2C_IFRAM_ADDR,
                bao1x_hal::board::I2C_IFRAM_ADDR,
                4096,
            )
        };
        let mut i2c = unsafe {
            bao1x_hal::udma::I2c::new_with_ifram(i2c_channel, 400_000, perclk, i2c_ifram, &udma_global)
        };

        if let Ok(mut pmic) = bao1x_hal::axp2101::Axp2101::new(&mut i2c) {
            match pmic.set_dcdc(&mut i2c, Some((0.88, true)), bao1x_hal::axp2101::WhichDcDc::Dcdc2) {
                Ok(_) => crate::println!("turned on DCDC2"),
                Err(_) => crate::println!("couldn't turn off DCDC2"),
            }
            pmic.set_pwm_mode(&mut i2c, bao1x_hal::axp2101::WhichDcDc::Dcdc2, true).ok();
        }
        // this does nothing on boards without the FET rework
        crate::println!("Engage DCDC2 FET");
        iox.set_gpio_pin_value(IoxPort::PA, 5, IoxValue::Low);
    }

    // code to setup PWM, for testing the PWM pin
    if false {
        let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
        iox.setup_pin(IoxPort::PF, 9, Some(IoxDir::Input), Some(IoxFunction::Gpio), None, None, None, None);
        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::Gpio),
            None,
            None,
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
        );
        iox.setup_pin(
            IoxPort::PA,
            0,
            Some(IoxDir::Output),
            Some(IoxFunction::AF3),
            None,
            None,
            Some(IoxEnable::Disable),
            Some(IoxDriveStrength::Drive8mA),
        );
        let mut timer = CSR::new(utra::pwm::HW_PWM_BASE as *mut u32);
        timer.wo(utra::pwm::REG_CH_EN, 1);
        timer.rmwf(utra::pwm::REG_TIM0_CFG_R_TIMER0_SAW, 1);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_TH, 0);
        timer.rmwf(utra::pwm::REG_TIM0_CH0_TH_R_TIMER0_CH0_MODE, 3);
        let pwm = utra::pwm::HW_PWM_BASE as *mut u32;
        // unsafe { pwm.add(2).write_volatile(1 << 16) };
        unsafe { pwm.add(2).write_volatile(0) };
        timer.rmwf(utra::pwm::REG_TIM0_CMD_R_TIMER0_START, 1);
        crate::println!("PWM running on PA0?");
        for i in 0..12 {
            crate::println!("0x{:2x}: 0x{:08x}", i, unsafe { pwm.add(i).read_volatile() })
        }
        crate::println!("0x{:2x}: 0x{:08x}", 65, unsafe { pwm.add(65).read_volatile() });
        /*
        for i in 80..84 {
            crate::println!("0x{:2x}: 0x{:08x}", i, unsafe { pwm.add(i).read_volatile() })
        }
        */
        crate::println!("");
    }
}

pub fn setup_timer() {
    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    // not using interrupts, this will be polled by delay()
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);

    let ms = SYSTEM_TICK_INTERVAL_MS;
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    // load its values
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
    // enable the timer
    timer.wfo(utra::timer0::EN_EN, 0b1);
}

pub fn setup_alloc() {
    // Initialize the allocator with heap memory range
    crate::println!("Setting up heap @ {:x}-{:x}", HEAP_START, HEAP_START + HEAP_LEN);
    crate::println!("Stack @ {:x}-{:x}", HEAP_START + HEAP_LEN, RAM_BASE + RAM_SIZE);
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}

/// Delay with a given system clock frequency. Useful during power mode switching.
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

/// Delay function that delays a given number of milliseconds.
pub fn delay(ms: usize) {
    let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        // comment this out for testing on MPW
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}

mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

/// This takes in the FD input frequency (the frequency to be divided) in MHz
/// and the fd value, and returns the resulting divided frequency.
/// *not tested*
#[allow(dead_code)]
pub fn fd_to_clk(fd_in_mhz: u32, fd_val: u32) -> u32 { (fd_in_mhz * (fd_val + 1)) / 256 }

/// Takes in the FD input frequencyin MHz, and then the desired frequency.
/// Returns Some((fd value, deviation in *hz*, not MHz)) if the requirement is satisfiable
/// Returns None if the equation is ill-formed.
/// *not tested*
#[allow(dead_code)]
pub fn clk_to_fd(fd_in_mhz: u32, desired_mhz: u32) -> Option<(u32, i32)> {
    let platonic_fd: u32 = (desired_mhz * 256) / fd_in_mhz;
    if platonic_fd > 0 {
        let actual_fd = platonic_fd - 1;
        let actual_clk = fd_to_clk(fd_in_mhz, actual_fd);
        Some((actual_fd, desired_mhz as i32 - actual_clk as i32))
    } else {
        None
    }
}

/// Takes in the top clock in MHz, desired perclk in MHz, and returns a tuple of
/// (min cycle, fd, actual freq)
/// *tested*
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

// This function supercedes init_clock_asic() and needs to be back-ported
// into xous-core
// TODO:
//  - [ ] Case of clocks <= 48MHz: turn off PLL, divide directly from OSC
//  - [ ] Derive clock dividers from freq targets, instead of hard-coding them
//  - [ ] Maybe improve dividers to optimize hclk/iclk/pclk settings in lower power?
//  - [ ] Very maybe consider setting hclk/iclk/pclk in case of overclocking
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

    // perclk divider - set to divide by 16 off of an 800Mhz base. Only found on bao1x.
    // daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
    // perclk divider - set to divide by 8 off of an 800Mhz base. Only found on bao1x.
    // TODO: this only works for two clock settings. Broken @ 600. Need to fix this to instead derive
    // what pclk is instead of always reporting 100mhz
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

    if freq_hz >= 600_000_000 {
        crate::println!("setting vdd85 to 0.893v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421FF1);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    } else {
        crate::println!("setting vdd85 to 0.80v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421080);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    }
    crate::println!("...done");

    // DARIC_CGU->cgusel1 = 1; // 0: RC, 1: XTAL
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    // DARIC_CGU->cgufscr = FREQ_OSC_MHZ; // external crystal is 48MHz
    cgu.wo(sysctrl::SFR_CGUFSCR, FREQ_OSC_MHZ);
    // __DSB();
    // DARIC_CGU->cguset = 0x32UL;
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    // __DSB();

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    if freq_hz <= 1_000_000 {
        // DARIC_IPC->osc = freqHz;
        cgu.wo(sysctrl::SFR_IPCOSC, freq_hz);
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();
    }
    // switch to OSC
    //DARIC_CGU->cgusel0 = 0; // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    // __DSB();
    // DARIC_CGU->cguset = 0x32; // commit
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    //__DSB();

    if freq_hz <= 1_000_000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        // PD PLL
        // DARIC_IPC->lpen |= 0x02 ;
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        // for (uint32_t i = 0; i < 1024; i++){
        //    __NOP();
        //}
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

        // TODO very verbose
        //printf ("%s(%4" PRIu32 "MHz) M = %4" PRIu32 ", N = %4" PRIu32 ".%08" PRIu32 ", Q = %2lu, QDiv =
        // 0x%04" PRIx16 "\n",     __FUNCTION__, freqHz / 1000000, M, (uint32_t)(n_fxp24 >> 24),
        // (uint32_t)((uint64_t)(n_fxp24 & 0x00ffffff) * 100000000/(1UL <<24)), TBL_MUL[f16MHzLog2],
        // TBL_Q[f16MHzLog2]); DARIC_IPC->pll_mn = ((M << 12) & 0x0001F000) | ((n_fxp24 >> 24) &
        // 0x00000fff); // 0x1F598; // ??
        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
        // DARIC_IPC->pll_f = n_frac | ((0 == n_frac) ? 0 : (1UL << 24)); // ??
        cgu.wo(sysctrl::SFR_IPCPLLF, n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
        // DARIC_IPC->pll_q = TBL_Q[f16MHzLog2]; // ?? TODO select DIV for VCO freq
        cgu.wo(sysctrl::SFR_IPCPLLQ, TBL_Q[f16mhz_log2 as usize] as u32);
        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        //DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        // DARIC_IPC->ipc = (1 << 6) | (2 << 3) | (3);
        cgu.wo(sysctrl::SFR_IPCCR, (1 << 6) | (2 << 3) | (3));
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // DARIC_IPC->lpen &= ~0x02;
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);

        //__DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        // for (uint32_t i = 0; i < 1024; i++){
        //    __NOP();
        // }
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 2");

        //printf("read reg a0 : %08" PRIx32"\n", *((volatile uint32_t* )0x400400a0));
        //printf("read reg a4 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a4));
        //printf("read reg a8 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a8));

        // TODO wait/poll lock status?
        // DARIC_CGU->cgusel0 = 1; // clktop sel, 0:clksys, 1:clkpll0
        cgu.wo(sysctrl::SFR_CGUSEL0, 1);
        // __DSB();
        // DARIC_CGU->cguset = 0x32; // commit
        cgu.wo(sysctrl::SFR_CGUSET, 0x32);
        crate::println!("clocks set");

        // __DSB();

        // printf ("    MN: 0x%05x, F: 0x%06x, Q: 0x%04x\n",
        //     DARIC_IPC->pll_mn, DARIC_IPC->pll_f, DARIC_IPC->pll_q);
        // printf ("    LPEN: 0x%01x, OSC: 0x%04x, BIAS: 0x%04x,\n",
        //     DARIC_IPC->lpen, DARIC_IPC->osc, DARIC_IPC->ipc);
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

/// used to generate some test vectors
#[allow(dead_code)]
pub fn lfsr_next_u32(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}

pub fn clockset_wrapper(freq: u32) -> u32 {
    // reset the baud rate on the console UART
    let perclk = unsafe { crate::platform::init_clock_asic(freq) };
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    #[cfg(feature = "bao1x-evb")]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    #[cfg(not(feature = "bao1x-evb"))]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    let baudrate: u32 = crate::UART_BAUD;
    let freq: u32 = perclk / 2;
    udma_uart.set_baud(baudrate, freq);

    crate::println!("clock set done, perclk is {} MHz", perclk / 1_000_000);
    udma_uart.write("console up with clocks\r\n".as_bytes());

    perclk
}

#[allow(dead_code)]
pub unsafe fn low_power() -> u32 {
    const FREQ_OSC_MHZ: u32 = 48; // 48MHz

    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f3f); // fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f3f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f1f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f0f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0707); // pclk

    let (min_cycle, fd, perclk) = if let Some((min_cycle, fd, perclk)) = clk_to_per(48, 24) {
        daric_cgu
            .add(utra::sysctrl::SFR_CGUFDPER.offset())
            .write_volatile((min_cycle as u32) << 16 | (fd as u32) << 8 | fd as u32);
        crate::println!("perclk {}", perclk);
        (min_cycle, fd, perclk * 1_000_000)
    } else {
        crate::println!("couldn't find perclk solution");
        daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
        (3, 0xff, 48_000_000 / 4)
    };
    let perclk = perclk;

    // DARIC_CGU->cgusel1 = 1; // 0: RC, 1: XTAL
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    cgu.wo(sysctrl::SFR_IPCOSC, FREQ_OSC_MHZ * 1_000_000);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    // lower core voltage to 0.7v
    let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
    ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420002);
    // ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x1);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);

    // power down the PLL
    cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);
    cgu.wo(sysctrl::SFR_IPCCR, 0x53);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    for _ in 0..1024 {
        unsafe { core::arch::asm!("nop") };
    }
    crate::println!("PLL pd delay 1");

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

    // gates off
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x00);
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x00);
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x00);
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x00);
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
    let mut udmacore = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udmacore.wo(utra::udma_ctrl::REG_CG, 0x0000_000F); // lower four are the UART

    // reset the UART
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    #[cfg(feature = "bao1x-evb")]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    #[cfg(not(feature = "bao1x-evb"))]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    let baudrate: u32 = crate::UART_BAUD;
    let freq: u32 = perclk / 2;
    udma_uart.set_baud(baudrate, freq);

    crate::println!("Perclk solution: {:x}|{:x} -> {} MHz", min_cycle, fd, perclk / 1_000_000);
    crate::println!("powerdown: perclk is {} MHz", perclk / 1_000_000);
    udma_uart.write("powerdown with clocks\r\n".as_bytes());

    perclk
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyPress {
    Up,
    Down,
    Left,
    Right,
    Select,
    Home,
    Invalid,
    None,
}
#[allow(dead_code)]
pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [KeyPress; 4] {
    let mut key_presses: [KeyPress; 4] = [KeyPress::None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                crate::println!("Key press at ({}, {})", row, col);
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = match (row, col) {
                        (1, 3) => KeyPress::Left,
                        (1, 2) => KeyPress::Home,
                        (1, 0) => KeyPress::Right,
                        (0, 0) => KeyPress::Down,
                        (0, 2) => KeyPress::Up,
                        (0, 1) => KeyPress::Select,
                        _ => KeyPress::Invalid,
                    };
                    key_press_index += 1;
                }
            }
        }
        iox.set_gpio_pin_value(*port, *pin, IoxValue::High);
    }
    key_presses
}

#[derive(Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
#[cfg(feature = "trng-debug")]
#[repr(u32)]
pub enum TrngOpt {
    RngA = 0x0,
    RngB = 0x1_0000,
}

#[cfg(feature = "trng-debug")]
fn trng_stop(trng: &mut CSR<u32>, sce: &mut CSR<u32>, opt: TrngOpt) {
    trng.wo(utra::trng::SFR_AR_GEN, 0xa5); // trigger stop
    match opt {
        TrngOpt::RngB => {
            sce.wo(utra::sce_glbsfr::SFR_FFEN, !(1 << 5)); // disable fifo
        }
        TrngOpt::RngA => {
            sce.wo(utra::sce_glbsfr::SFR_FFEN, !(1 << 4)); // disable fifo
        }
    }
}

#[cfg(feature = "trng-debug")]
fn trng_raw_buf_ready(trng: &CSR<u32>) -> bool { trng.r(utra::trng::SFR_SR) & (1 << 28) != 0 }

#[allow(dead_code)]
#[cfg(feature = "trng-debug")]
pub fn trng_ro(
    crsrc: u32,
    crana: u32,
    post_proc: u32,
    opt: TrngOpt,
    chain0_l: u32,
    chain0_h: u32,
    chain1_l: u32,
    chain1_h: u32,
    buf: &mut [u32],
    do_raw: bool,
) {
    let mut trng = CSR::new(utralib::utra::trng::HW_TRNG_BASE as *mut u32);
    let mut sce = CSR::new(utralib::utra::sce_glbsfr::HW_SCE_GLBSFR_BASE as *mut u32);
    let rngb_mem = unsafe {
        core::slice::from_raw_parts(
            utralib::HW_SEG_RNGB_MEM as *const u32,
            utralib::HW_SEG_RNGB_MEM_LEN / size_of::<u32>(),
        )
    };
    let rnga_mem = unsafe {
        core::slice::from_raw_parts(
            utralib::HW_SEG_RNGA_MEM as *const u32,
            utralib::HW_SEG_RNGA_MEM_LEN / size_of::<u32>(),
        )
    };

    match opt {
        TrngOpt::RngB => sce.wo(utra::sce_glbsfr::SFR_FFEN, 1 << 5),
        TrngOpt::RngA => sce.wo(utra::sce_glbsfr::SFR_FFEN, 1 << 4),
    }

    trng.wo(utra::trng::SFR_CRSRC, crsrc);
    trng.wo(utra::trng::SFR_CRANA, crana);
    trng.wo(utra::trng::SFR_PP, post_proc);
    trng.wo(utra::trng::SFR_OPT, opt as u32 | 0x100);
    // contex.trng->opt = 0x10040;   // use rngB , gen 0x40*4*4=1024 bytes 为啥现在是 0x100 *4 ???
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN0, chain0_l);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN1, chain0_h);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN2, chain1_l);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN3, chain1_h);

    /*
    crate::println!("crsrc: {:08x}", trng.r(utra::trng::SFR_CRSRC));
    crate::println!("crana: {:08x}", trng.r(utra::trng::SFR_CRANA));
    crate::println!("postproc: {:08x}", trng.r(utra::trng::SFR_PP));
    crate::println!("opt: {:08x}", trng.r(utra::trng::SFR_OPT));
    crate::println!("chain0_l: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN0));
    crate::println!("chain0_h: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN1));
    crate::println!("chain1_l: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN2));
    crate::println!("chain1_h: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN3));

    crate::println!("rng start");
    */
    if !do_raw {
        for (genloop, chunk) in buf.chunks_mut(256).enumerate() {
            match opt {
                TrngOpt::RngB => sce.wo(utra::sce_glbsfr::SFR_FFCLR, 0x0000FF05),
                TrngOpt::RngA => sce.wo(utra::sce_glbsfr::SFR_FFCLR, 0x0000FF04),
            }

            if genloop != 0 {
                /*
                crate::println!(
                    "GEN_LOOP = {},HLTHTEST_ERRCNT (After gen)= {}",
                    genloop - 1,
                    ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                ); //上一次结果打印
                */
            }
            /*
            crate::println!(
                "GEN_LOOP = {} ,HLTHTEST_ERRCNT (Before gen)= {}",
                genloop,
                ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
            ); //新的起始打印
            */
            trng.wo(utra::trng::SFR_AR_GEN, 0x5a); // trigger start

            match opt {
                TrngOpt::RngB => {
                    while (sce.r(utra::sce_glbsfr::SFR_FFCNT_SR_FF5) >> 4) & 0xFFF < chunk.len() as u32 {
                        // wait
                    }
                    chunk.copy_from_slice(&rngb_mem[..chunk.len()]);
                    /*
                    crate::println!(
                        "GEN_LOOP = {} ,HLTHTEST_ERRCNT (After gen)= {}\r\n",
                        genloop,
                        ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                    );
                    */
                }
                TrngOpt::RngA => {
                    while (sce.r(utra::sce_glbsfr::SFR_FFCNT_SR_FF4) >> 4) & 0xFFF < chunk.len() as u32 {
                        // wait
                    }
                    chunk.copy_from_slice(&rnga_mem[..chunk.len()]);
                    /*
                    crate::println!(
                        "GEN_LOOP = {} ,HLTHTEST_ERRCNT (After gen)= {}\r\n",
                        genloop,
                        ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                    );
                    */
                }
            }
        }
        // crate::println!("HLTHTEST_ERRCNT (After gen)= {}\r\n", ((trng.r(utra::trng::SFR_SR) >> 16) &
        // 0x00FF));
        trng_stop(&mut trng, &mut sce, opt);
    } else {
        // make sure the trng is in the stopped state
        trng_stop(&mut trng, &mut sce, opt);
        // crate::println!("raw data");

        for d in buf.iter_mut() {
            while !trng_raw_buf_ready(&trng) {} // wait for buffer to fill
            *d = trng.r(utra::trng::SFR_BUF);
        }
    }
    // crate::println!("rng stop");
}
