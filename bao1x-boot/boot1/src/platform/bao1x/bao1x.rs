use bao1x_api::*;
use bao1x_hal::{board::KeyPress, iox::Iox, udma::GlobalConfig};
use utralib::CSR;
use utralib::utra;

use crate::platform::{
    debug::setup_console,
    irq::{enable_irq, irq_setup},
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const SIGBLOCK_LEN: usize = 768; // this is adjusted inside builder.rs, in the sign-image invocation

const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

// scratch page for exceptions
//   - scratch data is stored in positive offsets from here
//   - exception stack is stored in negative offsets from here, hence the +4096
// total occupied area is [HEAP_START + HEAP_LEN..HEAP_START + HEAP_LEN + 8192]
pub const SCRATCH_PAGE: usize = HEAP_START + HEAP_LEN + 4096;

pub const FREE_MEM_START: usize = SCRATCH_PAGE + 16384;
pub const STACK_LEN: usize = 128 * 1024; // 128k for stack is more than enough (usually <16k)
pub const FREE_MEM_LEN: usize = (RAM_BASE + RAM_SIZE) - FREE_MEM_START - STACK_LEN;

// NOTE: this forces the mapping to be the same on both baosec and dabao
pub const UART_IFRAM_ADDR: usize = bao1x_hal::board::UART_DMA_TX_BUF_PHYS;

const SAFE_FCLK_FREQUENCY: u32 = 350_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

// Dabao port/pin constants have to be vendored in because this crate is compiled with baosec as the target.
const DABAO_SE0_PIN: u8 = 13;
const DABAO_SE0_PORT: IoxPort = IoxPort::PC;

pub fn setup_dabao_boot_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    iox.setup_pin(
        DABAO_SE0_PORT,
        DABAO_SE0_PIN,
        Some(IoxDir::Input),
        Some(IoxFunction::Gpio),
        Some(IoxEnable::Enable), // enable the schmitt trigger on this pad
        Some(IoxEnable::Enable), // enable the pullup
        None,
        None,
    );
    (DABAO_SE0_PORT, DABAO_SE0_PIN)
}

pub fn setup_dabao_se0_pin<T: IoSetup + IoGpio>(iox: &T) -> (IoxPort, u8) {
    iox.setup_pin(
        DABAO_SE0_PORT,
        DABAO_SE0_PIN,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Enable),
        Some(IoxEnable::Enable),
        Some(IoxDriveStrength::Drive2mA),
    );
    (DABAO_SE0_PORT, DABAO_SE0_PIN)
}

/// This can change the board type coding to a safer, simpler board type if the declared board type has
/// problems booting.
pub fn early_init(mut board_type: bao1x_api::BoardTypeCoding) -> (bao1x_api::BoardTypeCoding, u32) {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

    // setup board-specific I/Os - early boot set. These are items that have to be
    // done in a time-sensitive fashion.
    match board_type {
        BoardTypeCoding::Dabao | BoardTypeCoding::Oem => {
            // setup the dabao 'boot' read pin for reading. This also connects the USB port temporarily.
            setup_dabao_boot_pin(&iox);

            // setup the RAMs for our trim voltage
            let trim_table = bao1x_hal::sram_trim::get_sram_trim_for_voltage(
                bao1x_api::offsets::dabao::CPU_VDD_LDO_BOOT_MV,
            );
            let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
            for item in trim_table {
                rbist.wo(utra::rbist_wrp::SFRCR_TRM, item.raw_value());
                rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
            }
        }
        BoardTypeCoding::Baosec => {
            let i2c_channel = bao1x_hal::board::setup_i2c_pins(&iox);
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
                bao1x_hal::udma::I2c::new_with_ifram(
                    i2c_channel,
                    400_000,
                    100_000_000,
                    i2c_ifram,
                    &udma_global,
                )
            };
            // Try a couple times to talk to the PMIC - just in case there's a weird glitch in the power-on
            let mut pmic_ok = false;
            for _ in 0..2 {
                match bao1x_hal::axp2101::Axp2101::new(&mut i2c) {
                    Ok(mut pmic) => {
                        pmic_ok = true;
                        // it is timing-critical to enable BLDO1. This is what keeps the battery-enable switch
                        // connected. The idea is that the user has to hold down the "on" switch until this
                        // line of code goes through, for there to be a durable power-on. The booting time
                        // serves as a "debounce" of accidental butt-dials of the
                        // power button.
                        pmic.set_ldo(&mut i2c, Some(3.3), bao1x_hal::axp2101::WhichLdo::Bldo1).unwrap();

                        // set PWM mode on DCDC2. greatly reduces noise on the regulator line
                        pmic.set_pwm_mode(&mut i2c, bao1x_hal::axp2101::WhichDcDc::Dcdc2, true).unwrap();
                        // make sure the DCDC2 is set to 0.9V, which will allow us to enter high-speed run
                        // mode. It defaults to 0.85V on boot.
                        pmic.set_dcdc(&mut i2c, Some((0.9, true)), bao1x_hal::axp2101::WhichDcDc::Dcdc2)
                            .unwrap();

                        break;
                    }
                    Err(e) => {
                        crate::println!("Error initializing pmic: {:?}, retrying", e);
                    }
                };
            }
            if !pmic_ok {
                // fallback into a dabao-type of configuration. This is safe even on baosec but
                // allows for debugging/RMA processing.
                board_type = BoardTypeCoding::Dabao;
                // setup the dabao 'boot' read pin for reading. This also connects the USB port temporarily.
                setup_dabao_boot_pin(&iox);

                // we're running at 0.8V, setup the RAMs for that
                let trim_table = bao1x_hal::sram_trim::get_sram_trim_for_voltage(
                    bao1x_api::offsets::dabao::CPU_VDD_LDO_BOOT_MV,
                );
                let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
                for item in trim_table {
                    rbist.wo(utra::rbist_wrp::SFRCR_TRM, item.raw_value());
                    rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
                }
            } else {
                // remainder of setup for baosec
                let (port, pin) = bao1x_hal::board::setup_dcdc2_pin(&iox);
                // low connects DCDC2 to the chip
                iox.set_gpio_pin(port, pin, IoxValue::Low);

                // TODO: I *think* these four registers are actually not connected and setting the
                // values do nothing. But, the bring-up team is overwhelmed
                // and I feel bad pestering them about this. This is a note to
                // ask questions later when the temperature is a bit lower.
                let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
                sramtrm.wo(utra::coresub_sramtrm::SFR_CACHE, 0x3);
                sramtrm.wo(utra::coresub_sramtrm::SFR_ITCM, 0x3);
                sramtrm.wo(utra::coresub_sramtrm::SFR_DTCM, 0x3);
                sramtrm.wo(utra::coresub_sramtrm::SFR_VEXRAM, 0x1);

                // we should be in 0.9v mode, setup SRAM trimmings for that
                let trim_table = bao1x_hal::sram_trim::get_sram_trim_for_voltage(
                    bao1x_api::offsets::baosec::CPU_VDD_LDO_BOOT_MV,
                );
                let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
                for item in trim_table {
                    rbist.wo(utra::rbist_wrp::SFRCR_TRM, item.raw_value());
                    rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
                }

                let (se0_port, se0_pin) = bao1x_hal::board::setup_usb_pins(&iox);
                iox.set_gpio_pin(se0_port, se0_pin, IoxValue::Low); // put the USB port into SE0 while we initialize things

                // setup display - turn on its power, reset the framebuffer
                bao1x_hal::board::setup_display_pins(&iox);
                // power on
                let (oled_on_port, oled_on_pin) = bao1x_hal::board::setup_oled_power_pin(&iox);
                iox.set_gpio_pin_value(oled_on_port, oled_on_pin, IoxValue::High);
                // reset enable
                let (peri_rst_port, peri_reset_pin) = bao1x_hal::board::setup_periph_reset_pin(&iox);
                iox.set_gpio_pin_value(peri_rst_port, peri_reset_pin, IoxValue::Low);
                // delay for reset assert
                delay(1);
                iox.set_gpio_pin_value(peri_rst_port, peri_reset_pin, IoxValue::High);

                // keyboard can setup at keyboard read time
            }
        }
    }

    // ASSUME: basic clocks are sane because boot0 set those up.
    // ASSUME: SRAM trim wait states are set up correctly (sram0 has 1 wait state)

    // Now that SRAM trims are setup, initialize all the statics by writing to memory.
    // For baremetal, the statics structure is just at the flash base.
    const STATICS_LOC: usize = bao1x_api::BOOT1_START + SIGBLOCK_LEN;

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
            let offset = u16::from_le_bytes(offset) as usize;
            let data = u32::from_le_bytes(data);
            data_ptr.add(offset / size_of::<u32>()).write_volatile(data);
        }
    }

    // set the clock
    let fclk_freq = match board_type {
        BoardTypeCoding::Baosec => bao1x_api::offsets::baosec::DEFAULT_FCLK_FREQUENCY,
        BoardTypeCoding::Oem => SAFE_FCLK_FREQUENCY,
        BoardTypeCoding::Dabao => SAFE_FCLK_FREQUENCY,
    };
    let perclk = unsafe { init_clock_asic(fclk_freq) };

    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);

    // setup heap alloc
    setup_alloc();

    setup_timer(fclk_freq);

    // check key slots for integrity. Has to be done late in boot because we might
    // have to generate keys, and that requires a lot of stuff to be working correctly:
    // in particular, we'll need `alloc` (to store the random vectors) and `timer`
    // (to detect disconnected/failed TRNG).
    let mut cu = bao1x_hal::coreuser::Coreuser::new();
    // Coreuser needs to be set up correctly for check_slots to succeed.
    cu.set();
    crate::platform::slots::check_slots(&board_type);
    // protect() is called inside sigcheck on boot!

    // Rx setup
    let _udma_uart = setup_console(&board_type, &iox, perclk);
    irq_setup();
    enable_irq(utra::irqarray5::IRQARRAY5_IRQ);

    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);
    crate::println!("boot1 udma console up, CPU @ {}MHz!", fclk_freq / 2_000_000);

    (board_type, perclk)
}

pub fn setup_timer(sysclk_freq: u32) {
    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    // not using interrupts, this will be polled by delay()
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);

    let ms = SYSTEM_TICK_INTERVAL_MS;
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    // load its values
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, (sysclk_freq / 1_000) * ms);
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
#[allow(dead_code)]
pub fn delay(ms: usize) {
    let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
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

pub fn get_key<T: IoSetup + IoGpio>(board_type: &BoardTypeCoding, iox: &T) -> Option<KeyPress> {
    // check key press state. Depends on the board type
    match board_type {
        BoardTypeCoding::Baosec => {
            let (rows, cols) = bao1x_hal::board::setup_kb_pins(iox);
            let kps = bao1x_hal::board::scan_keyboard(iox, &rows, &cols);
            // record which key is pressed
            if kps[0] != KeyPress::None { Some(kps[0]) } else { None }
        }
        _ => {
            let (port, pin) = crate::platform::setup_dabao_boot_pin(iox);
            // sample the pin
            if iox.get_gpio_pin_value(port, pin) == IoxValue::Low {
                // "borrow" the dabao keypress meaning for this pin
                Some(KeyPress::Select)
            } else {
                None
            }
        }
    }
}
