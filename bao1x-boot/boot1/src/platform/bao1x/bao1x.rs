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

#[cfg(not(feature = "alt-boot1"))]
pub fn setup_backup_region() -> u32 {
    let mut bu_mgr = bao1x_hal::buram::BackupManager::new();
    if !bu_mgr.is_backup_valid() {
        // zeroize the hashable backup RAM area
        // safety: make_valid is called after this is done.
        unsafe {
            bu_mgr.bu_hashable_ram_as_mut().fill(0);
        }
        // calculate the hash and mark as valid
        bu_mgr.make_valid();

        // setup the BIO, so the reset can also clear its registers and state for a clean BDMA pipeline
        let mut bio_ss = xous_bio_bdma::BioSharedState::new();
        bio_ss.init();
        // must disable DMA filtering
        bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_MEM, 1);
        bio_ss.bio.rmwf(utra::bio_bdma::SFR_CONFIG_DISABLE_FILTER_PERI, 1);

        // stop all the machines, so that code can be loaded
        bio_ss.bio.wo(utra::bio_bdma::SFR_CTRL, 0x0);
        // reset all the fifos
        bio_ss.bio.wo(utra::bio_bdma::SFR_FIFO_CLR, 0xF);
        // setup clocking mode option
        bio_ss.bio.rmwf(utralib::utra::bio_bdma::SFR_CONFIG_CLOCKING_MODE, 3 as u32);

        bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV0, 0x1_0000);
        bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV1, 0x1_0000);
        bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV2, 0x1_0000);
        bio_ss.bio.wo(utra::bio_bdma::SFR_QDIV3, 0x1_0000);

        let mut trng = bao1x_hal::sce::trng::Trng::new(utralib::utra::trng::HW_TRNG_BASE);
        trng.setup_raw_generation(256);
        trng.start();
        let mut delay: u16 = 0;
        for _ in 0..8 {
            delay ^= trng.get_u32().unwrap() as u16;
        }
        let mut acc = 1;
        // one of ~1k slots for delay
        for i in 0..delay & 0x3FF {
            acc += i as u32;
        }

        // enable the RTC if it isn't already - on a cold boot, it would be off
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        // set effective tick to 1/1024th of a second
        // this causes the RTC to roll over every 48 days - the code before had some deniability about
        // RTC offset because it would roll over every 168 years, but I think once every 48 days is within
        // a reasonable window of deniability (previously it was a random number from 0-6 months) and
        // also the system has to be powered on at least once every rollover window to capture the rollover.
        // The device won't last 48 days on RTC battery alone - and every time a QR code is scanned you
        // update the RTC offset. So I think this is a reasonable window to just leave "as so". The
        // initial value is set to within minutes of a roll-over, so that an error in rollover handling
        // would be detected quickly.
        ao_sysctrl.wo(utra::ao_sysctrl::CR_CLK1HZFD, 15);
        let mut rtc = CSR::new(bao1x_hal::rtc::HW_RTC_BASE as *mut u32);
        rtc.wo(bao1x_hal::rtc::LR, 0xFFFE_2000); // set base to "roll over" in 2 minutes - forces an edge case in testing
        rtc.wfo(bao1x_hal::rtc::CR_EN, 1);

        // soft-reset the system
        let mut rcurst = CSR::new(utra::sysctrl::HW_SYSCTRL_BASE as *mut u32);
        rcurst.wo(utra::sysctrl::SFR_RCURST0, 0x55AA);

        // this is never returned, but guarantees the compiler does not optimize out the delay loop
        acc
    } else {
        0
    }
}

/// This can change the board type coding to a safer, simpler board type if the declared board type has
/// problems booting.
pub fn early_init(mut board_type: bao1x_api::BoardTypeCoding) -> (bao1x_api::BoardTypeCoding, u32) {
    // This can be used to debug stuff early on dabao, assuming boot0 setup the console UART. It normally
    // does, but we leave this commented out for non-debug situations because it's a weak guarantee.
    // crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);

    irq_setup();
    // all sensors & IRQs setup already by boot0, but it doesn't hurt to re-write these registers
    // in case they were defeated somehow previously
    let mut irq13 = CSR::new(utra::irqarray13::HW_IRQARRAY13_BASE as *mut u32);
    irq13.wo(utra::irqarray13::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
    irq13.wo(utra::irqarray13::EV_POLARITY, 0xFFFF_FFFF);
    irq13.wo(utra::irqarray13::EV_ENABLE, 0xFFFF_FFFF);
    // enable the IRQ because it was disabled by the previous stage's exit
    enable_irq(utra::irqarray13::IRQARRAY13_IRQ);

    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

    // this has to happen after the IRQs are enabled because if a false security alarm is triggered the
    // system won't reboot properly at the end of the set_backup_region() routine
    #[cfg(not(feature = "alt-boot1"))]
    if setup_backup_region() == 0 {
        crate::println!("backup region is clean!");
    }

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
                bao1x_hal::udma::I2cDriver::new_with_ifram(
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
                        // make sure the DCDC2 is set. Target 20mV above the acceptable run threshold because
                        // we have to take into account the transistor loss on the
                        // power switch.
                        pmic.set_dcdc(
                            &mut i2c,
                            Some((
                                (bao1x_hal::board::DEFAULT_CPU_VOLTAGE_MV
                                    + bao1x_hal::board::VDD85_SWITCH_MARGIN_MV)
                                    as f32
                                    / 1000.0,
                                true,
                            )),
                            bao1x_hal::axp2101::WhichDcDc::Dcdc2,
                        )
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
        BoardTypeCoding::Dabao => bao1x_api::offsets::dabao::DEFAULT_FCLK_FREQUENCY,
    };
    let perclk = unsafe {
        bao1x_hal::clocks::init_clock_asic(
            fclk_freq,
            utra::sysctrl::HW_SYSCTRL_BASE,
            utralib::HW_AO_SYSCTRL_BASE,
            Some(utra::duart::HW_DUART_BASE),
            delay_at_sysfreq,
            // slow BIO for better power savings. Overridden in baremetal & dabao images for faster
            // performance since these are plug-in applications.
            false,
        )
    };

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
    // can't check slots in alt-boot mode because it's effectively baremetal as far as permissions go
    #[cfg(not(feature = "alt-boot1"))]
    crate::platform::slots::check_slots(&board_type);
    // protect() is called inside sigcheck on boot!

    // Rx setup
    let _udma_uart = setup_console(&board_type, &iox, perclk);
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

    let ms = 1;
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
