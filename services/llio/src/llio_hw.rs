
use crate::api::*;
use log::{error, info};
use utralib::{generated::*, utra::gpio::UARTSEL_UARTSEL};
use num_traits::ToPrimitive;
use susres::{RegManager, RegOrField, SuspendResume};

#[allow(dead_code)]
pub struct Llio {
    gpio_csr: utralib::CSR<u32>,
    gpio_susres: RegManager::<{utra::gpio::GPIO_NUMREGS}>,
    info_csr: utralib::CSR<u32>,
    handler_conn: Option<xous::CID>,
    event_csr: utralib::CSR<u32>,
    event_susres: RegManager::<{utra::btevents::BTEVENTS_NUMREGS}>,
    power_csr: utralib::CSR<u32>,
    power_csr_raw: *mut u32,
    power_susres: RegManager::<{utra::power::POWER_NUMREGS}>,
    xadc_csr: utralib::CSR<u32>,  // be careful with this as XADC is shared with TRNG
    ticktimer: ticktimer_server::Ticktimer,
    activity_period: u32, // 12mhz clock cycles over which to sample activity
    destruct_armed: bool,
    uartmux_cache: u32, // stash a value of the uartmux -- restore from override into kernel so we can record KPs on resume
}

fn handle_event_irq(_irq_no: usize, arg: *mut usize) {
    let xl = unsafe { &mut *(arg as *mut Llio) };
    if xl.event_csr.rf(utra::btevents::EV_PENDING_COM_INT) != 0 {
        if let Some(conn) = xl.handler_conn {
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::EventComHappened.to_usize().unwrap(), 0, 0, 0, 0)).map(|_|()).unwrap();
        } else {
            log::error!("|handle_event_irq: COM interrupt, but no connection for notification!")
        }
    }
    xl.event_csr
        .wo(utra::btevents::EV_PENDING, xl.event_csr.r(utra::btevents::EV_PENDING));
}
fn handle_gpio_irq(_irq_no: usize, arg: *mut usize) {
    let xl = unsafe { &mut *(arg as *mut Llio) };
    if let Some(conn) = xl.handler_conn {
        xous::try_send_message(conn,
            xous::Message::new_scalar(Opcode::GpioIntHappened.to_usize().unwrap(),
                xl.gpio_csr.r(utra::gpio::EV_PENDING) as _, 0, 0, 0)).map(|_|()).unwrap();
    } else {
        log::error!("|handle_event_irq: GPIO interrupt, but no connection for notification!")
    }
    xl.gpio_csr
        .wo(utra::gpio::EV_PENDING, xl.gpio_csr.r(utra::gpio::EV_PENDING));
}
fn handle_power_irq(_irq_no: usize, arg: *mut usize) {
    let xl = unsafe { &mut *(arg as *mut Llio) };
    if xl.power_csr.rf(utra::power::EV_PENDING_USB_ATTACH) != 0 {
        if let Some(conn) = xl.handler_conn {
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::EventUsbHappened.to_usize().unwrap(),
                    0, 0, 0, 0)).map(|_|()).unwrap();
        } else {
            log::error!("|handle_event_irq: USB interrupt, but no connection for notification!")
        }
    } else if xl.power_csr.rf(utra::power::EV_PENDING_ACTIVITY_UPDATE) != 0 {
        if let Some(conn) = xl.handler_conn {
            let activity = xl.power_csr.rf(utra::power::ACTIVITY_RATE_COUNTS_AWAKE);
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::EventActivityHappened.to_usize().unwrap(),
                    activity as usize, 0, 0, 0)).map(|_|()).unwrap();
        } else {
            log::error!("|handle_event_irq: activity interrupt, but no connection for notification!")
        }
    }
    xl.power_csr
        .wo(utra::power::EV_PENDING, xl.power_csr.r(utra::power::EV_PENDING));
}

pub fn log_init() -> *mut u32 {
    let gpio_base = xous::syscall::map_memory(
        xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map GPIO CSR range");
    let mut gpio_csr = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
    // setup the initial logging output
    gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, crate::api::BOOT_UART);

    gpio_base.as_mut_ptr() as *mut u32
}

impl Llio {
    pub fn new(handler_conn: xous::CID, gpio_base: *mut u32) -> Llio {
        let info_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::info::HW_INFO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Info CSR range");
        let event_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::btevents::HW_BTEVENTS_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map BtEvents CSR range");
        let power_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::power::HW_POWER_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Power CSR range");
        let power_csr_raw = power_csr.as_mut_ptr() as *mut u32;
        let xadc_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::trng::HW_TRNG_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map Xadc CSR range"); // note that Xadc is "in" the TRNG because TRNG can override Xadc in hardware

        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

        let mut xl = Llio {
            gpio_csr: CSR::new(gpio_base),
            gpio_susres: RegManager::new(gpio_base),
            info_csr: CSR::new(info_csr.as_mut_ptr() as *mut u32),
            handler_conn: Some(handler_conn), // connection for messages from IRQ handler
            event_csr: CSR::new(event_csr.as_mut_ptr() as *mut u32),
            event_susres: RegManager::new(event_csr.as_mut_ptr() as *mut u32),
            power_csr: CSR::new(power_csr_raw),
            power_csr_raw,
            power_susres: RegManager::new(power_csr.as_mut_ptr() as *mut u32),
            xadc_csr: CSR::new(xadc_csr.as_mut_ptr() as *mut u32),
            ticktimer,
            activity_period: 24_000_000, // 2 second interval initially
            destruct_armed: false,
            uartmux_cache: BOOT_UART.into(),
        };

        xous::claim_interrupt(
            utra::btevents::BTEVENTS_IRQ,
            handle_event_irq,
            (&mut xl) as *mut Llio as *mut usize,
        )
        .expect("couldn't claim BtEvents irq");

        xous::claim_interrupt(
            utra::gpio::GPIO_IRQ,
            handle_gpio_irq,
            (&mut xl) as *mut Llio as *mut usize,
        )
        .expect("couldn't claim GPIO irq");

        xous::claim_interrupt(
            utra::power::POWER_IRQ,
            handle_power_irq,
            (&mut xl) as *mut Llio as *mut usize,
        )
        .expect("couldn't claim Power irq");

        xl.gpio_susres.push(RegOrField::Reg(utra::gpio::DRIVE), None);
        xl.gpio_susres.push(RegOrField::Reg(utra::gpio::OUTPUT), None);
        xl.gpio_susres.push(RegOrField::Reg(utra::gpio::INTPOL), None);
        xl.gpio_susres.push(RegOrField::Reg(utra::gpio::INTENA), None);
        xl.gpio_susres.push_fixed_value(RegOrField::Reg(utra::gpio::EV_PENDING), 0xFFFF_FFFF);
        xl.gpio_susres.push(RegOrField::Reg(utra::gpio::EV_ENABLE), None);
        xl.gpio_susres.push(RegOrField::Field(utra::gpio::UARTSEL_UARTSEL), None);

        xl.event_susres.push_fixed_value(RegOrField::Reg(utra::btevents::EV_PENDING), 0xFFFF_FFFF);
        xl.event_susres.push(RegOrField::Reg(utra::btevents::EV_ENABLE), None);

        xl.power_csr.rmwf(utra::power::POWER_CRYPTO_ON, 0); // save power on crypto block
        xl.power_susres.push(RegOrField::Reg(utra::power::POWER), None);
        xl.power_susres.push(RegOrField::Reg(utra::power::VIBE), None);

        xl.power_csr.wfo(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, xl.activity_period); // 2 second sampling intervals
        xl.power_susres.push(RegOrField::Reg(utra::power::SAMPLING_PERIOD), None);
        xl.power_csr.wfo(utra::power::EV_PENDING_ACTIVITY_UPDATE, 1);
        xl.power_csr.rmwf(utra::power::EV_ENABLE_ACTIVITY_UPDATE, 1);

        xl.power_susres.push_fixed_value(RegOrField::Reg(utra::power::EV_PENDING), 0xFFFF_FFFF);
        xl.power_susres.push(RegOrField::Reg(utra::power::EV_ENABLE), None);

        xl
    }
    pub(crate) fn get_power_csr_raw(&self) -> *mut u32 {
        self.power_csr_raw
    }
    pub fn suspend(&mut self) {
        self.uartmux_cache = self.gpio_csr.rf(UARTSEL_UARTSEL).into();
        self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // set to console to watch on boot: 0 = kernel, 1 = console, 2 = application

        self.event_susres.suspend();
        // this happens after suspend, so these disables are "lost" upon resume and replaced with the normal running values
        self.event_csr.wo(utra::btevents::EV_ENABLE, 0);
        self.event_csr.wo(utra::btevents::EV_PENDING, 0xFFFF_FFFF); // really make sure we don't have any spurious events in the queue

        self.gpio_susres.suspend();
        self.gpio_csr.wo(utra::gpio::EV_ENABLE, 0);
        self.gpio_csr.wo(utra::gpio::EV_PENDING, 0xFFFF_FFFF);

        self.power_susres.suspend();
        self.power_csr.wo(utra::power::EV_ENABLE, 0);
        self.power_csr.wo(utra::power::EV_PENDING, 0xFFFF_FFFF);
    }
    pub fn resume(&mut self) {
        self.power_susres.resume();

        // reset these to "on" in case the "off" value was captured and stored on suspend
        // (these "should" be redundant)
        self.power_csr.rmwf(utra::power::POWER_SELF, 1);
        self.power_csr.rmwf(utra::power::POWER_STATE, 1);
        self.power_csr.rmwf(utra::power::POWER_UP5K_ON, 1);

        self.event_susres.resume();
        self.gpio_susres.resume();
        // restore the UART mux setting after resume
        self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, self.uartmux_cache);
    }
    #[allow(dead_code)]
    pub fn activity_set_period(&mut self, period: u32) {
        self.activity_period =  period;
        self.power_csr.wfo(utra::power::SAMPLING_PERIOD_SAMPLE_PERIOD, period);
    }
    pub fn activity_get_period(&mut self) -> u32 {
        self.activity_period
    }

    pub fn gpio_dout(&mut self, d: u32) {
        self.gpio_csr.wfo(utra::gpio::OUTPUT_OUTPUT, d);
    }
    pub fn gpio_din(&self) -> u32 {
        self.gpio_csr.rf(utra::gpio::INPUT_INPUT)
    }
    pub fn gpio_drive(&mut self, d: u32) {
        self.gpio_csr.wfo(utra::gpio::DRIVE_DRIVE, d);
    }
    pub fn gpio_int_mask(&mut self, d: u32) {
        self.gpio_csr.wfo(utra::gpio::INTENA_INTENA, d);
    }
    pub fn gpio_int_as_falling(&mut self, d: u32) {
        self.gpio_csr.wfo(utra::gpio::INTPOL_INTPOL, d);
    }
    pub fn gpio_int_pending(&self) -> u32 {
        self.gpio_csr.r(utra::gpio::EV_PENDING) & 0xff
    }
    pub fn gpio_int_ena(&mut self, d: u32) {
        self.gpio_csr.wo(utra::gpio::EV_ENABLE, d & 0xff);
    }
    pub fn set_uart_mux(&mut self, mux: UartType) {
        match mux {
            UartType::Kernel => {
                log::warn!("disabling WFI so that kernel console works as expected");
                self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 1);
                self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 0);
            },
            UartType::Log => {
                // this is a command mainly for debugging, so we'll accept the chance that we re-enabled WFI e.g.
                // during a critical operation like SPINOR flashing because we swapped consoles at a bad time. Should be
                // very rare and only affect devs...
                log::warn!("unsafe re-enabling WFI -- if you issued this command at a bad time, could have side effects");
                if true {
                    self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 0);
                } else {
                    log::warn!("sticking WFI override to 1 for keyboard debug, remove this path when done!");
                    self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 1);
                }
                self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1)
            },
            UartType::Application => {
                log::warn!("disabling WFI so that app console works as expected");
                self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 1);
                self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 2)
            },
            _ => info!("invalid UART type specified for mux, doing nothing."),
        }
    }

    pub fn get_info_dna(&self) -> (usize, usize) {
        (self.info_csr.r(utra::info::DNA_ID0) as usize, self.info_csr.r(utra::info::DNA_ID1) as usize)
    }
    pub fn get_info_git(&self) -> (usize, usize) {
        (
            ((self.info_csr.rf(utra::info::GIT_MAJOR_GIT_MAJOR) as u32) << 24 |
            (self.info_csr.rf(utra::info::GIT_MINOR_GIT_MINOR) as u32) << 16 |
            (self.info_csr.rf(utra::info::GIT_REVISION_GIT_REVISION) as u32) << 8 |
            (self.info_csr.rf(utra::info::GIT_GITEXTRA_GIT_GITEXTRA) as u32) & 0xFF << 0) as usize,

            self.info_csr.rf(utra::info::GIT_GITREV_GIT_GITREV) as usize
        )
    }
    pub fn get_info_platform(&self) -> (usize, usize) {
        (self.info_csr.r(utra::info::PLATFORM_PLATFORM0) as usize, self.info_csr.r(utra::info::PLATFORM_PLATFORM1) as usize)
    }
    pub fn get_info_target(&self) -> (usize, usize) {
        (self.info_csr.r(utra::info::PLATFORM_TARGET0) as usize, self.info_csr.r(utra::info::PLATFORM_TARGET1) as usize)
    }

    pub fn power_audio(&mut self, power_on: bool) {
        if power_on {
            self.power_csr.rmwf(utra::power::POWER_AUDIO, 1);
        } else {
            self.power_csr.rmwf(utra::power::POWER_AUDIO, 0);
        }
    }
    pub fn power_crypto(&mut self, power_on: bool) {
        if power_on {
            self.power_csr.rmwf(utra::power::POWER_CRYPTO_ON, 1);
        } else {
            self.power_csr.rmwf(utra::power::POWER_CRYPTO_ON, 0);
        }
    }
    // apparently "override" is a reserved keyword in Rust???
    pub fn wfi_override(&mut self, override_: bool) {
        if override_ {
            self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 1);
        } else {
            self.power_csr.rmwf(utra::power::POWER_DISABLE_WFI, 0);
        }
    }
    pub fn debug_powerdown(&mut self, ena: bool) {
        if ena {
            self.power_csr.rmwf(utra::gpio::DEBUG_WFI, 1);
        } else {
            self.power_csr.rmwf(utra::gpio::DEBUG_WFI, 0);
        }
    }
    pub fn debug_wakeup(&mut self, ena: bool) {
        if ena {
            self.power_csr.rmwf(utra::gpio::DEBUG_WAKEUP, 1);
        } else {
            self.power_csr.rmwf(utra::gpio::DEBUG_WAKEUP, 0);
        }
    }
    pub fn power_crypto_status(&self) -> (bool, bool, bool, bool) {
        let sha = if self.power_csr.rf(utra::power::CLK_STATUS_SHA_ON) == 0 {false} else {true};
        let engine = if self.power_csr.rf(utra::power::CLK_STATUS_ENGINE_ON) == 0 {false} else {true};
        let force = if self.power_csr.rf(utra::power::CLK_STATUS_BTPOWER_ON) == 0 {false} else {true};
        let overall = if self.power_csr.rf(utra::power::CLK_STATUS_CRYPTO_ON) == 0 {false} else {true};
        (overall, sha, engine, force)
    }
    pub fn power_self(&mut self, power_on: bool) {
        if power_on {
            info!("setting self-power state to on");
            self.power_csr.rmwf(utra::power::POWER_SELF, 1);
        } else {
            info!("setting self-power state to OFF");
            self.power_csr.rmwf(utra::power::POWER_STATE, 0);
            self.power_csr.rmwf(utra::power::POWER_SELF, 0);
        }
    }
    pub fn power_boost_mode(&mut self, boost_on: bool) {
        if boost_on {
            self.power_csr.rmwf(utra::power::POWER_BOOSTMODE, 1);
        } else {
            self.power_csr.rmwf(utra::power::POWER_BOOSTMODE, 0);
        }
    }
    pub fn ec_snoop_allow(&mut self, allow: bool) {
        if allow {
            self.power_csr.rmwf(utra::power::POWER_EC_SNOOP, 1);
        } else {
            self.power_csr.rmwf(utra::power::POWER_EC_SNOOP, 0);
        }
    }
    pub fn ec_reset(&mut self) {
        self.power_csr.rmwf(utra::power::POWER_UP5K_ON, 1); // make sure the power is "on" if we're resetting it
        self.power_csr.rmwf(utra::power::POWER_RESET_EC, 1);
        self.ticktimer.sleep_ms(20).unwrap();
        self.power_csr.rmwf(utra::power::POWER_RESET_EC, 0);
    }
    pub fn ec_power_on(&mut self) {
        self.power_csr.rmwf(utra::power::POWER_UP5K_ON, 1);
    }
    pub fn self_destruct(&mut self, code: u32) {
        if self.destruct_armed && code == 0x3141_5926 {
            self.ticktimer.sleep_ms(100).unwrap(); // give a moment for any last words (like clearing the screen, powering down, etc.)
            self.power_csr.rmwf(utra::power::POWER_SELFDESTRUCT, 1);
        } else if !self.destruct_armed && code == 0x2718_2818 {
            self.destruct_armed = true;
        } else {
            self.destruct_armed = false;
            error!("self destruct attempted, but incorrect code sequence presented.");
        }
    }
    pub fn vibe(&mut self, pattern: VibePattern) {
        match pattern {
            VibePattern::Short => {
                self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                self.ticktimer.sleep_ms(80).unwrap();
                self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
            },
            VibePattern::Long => {
                self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                self.ticktimer.sleep_ms(1000).unwrap();
                self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
            },
            VibePattern::Double => {
                self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                self.ticktimer.sleep_ms(150).unwrap();
                self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                self.ticktimer.sleep_ms(250).unwrap();
                self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                self.ticktimer.sleep_ms(150).unwrap();
                self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
            },
        }
    }
    /// this vibrates until the device sleeps
    #[allow(dead_code)]
    pub fn tts_sleep_indicate(&mut self) {
        self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
    }
    pub fn xadc_vbus(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_VBUS_XADC_VBUS) as u16
    }
    pub fn xadc_vccint(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_VCCINT_XADC_VCCINT) as u16
    }
    pub fn xadc_vccaux(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_VCCAUX_XADC_VCCAUX) as u16
    }
    pub fn xadc_vccbram(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_VCCBRAM_XADC_VCCBRAM) as u16
    }
    pub fn xadc_usbn(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_USB_N_XADC_USB_N) as u16
    }
    pub fn xadc_usbp(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_USB_P_XADC_USB_P) as u16
    }
    pub fn xadc_temperature(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_TEMPERATURE_XADC_TEMPERATURE) as u16
    }
    pub fn xadc_gpio5(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_GPIO5_XADC_GPIO5) as u16
    }
    pub fn xadc_gpio2(&self) -> u16 {
        self.xadc_csr.rf(utra::trng::XADC_GPIO2_XADC_GPIO2) as u16
    }
    pub fn com_int_ena(&mut self, ena: bool) {
        let value = if ena {1} else {0};
        self.event_csr.rmwf(utra::btevents::EV_ENABLE_COM_INT, value);
    }
    pub fn usb_int_ena(&mut self, ena: bool) {
        let value = if ena {1} else {0};
        self.power_csr.rmwf(utra::power::EV_ENABLE_USB_ATTACH, value);
    }
}

