#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod i2c;
mod rtc;

use num_traits::*;
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

use std::thread;

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    use crate::api::*;
    use log::{error, info};
    use utralib::{generated::*, utra::gpio::UARTSEL_UARTSEL};
    use num_traits::ToPrimitive;
    use susres::{RegManager, RegOrField, SuspendResume};

    #[allow(dead_code)]
    pub struct Llio {
        crg_csr: utralib::CSR<u32>,
        gpio_csr: utralib::CSR<u32>,
        gpio_susres: RegManager::<{utra::gpio::GPIO_NUMREGS}>,
        info_csr: utralib::CSR<u32>,
        identifier_csr: utralib::CSR<u32>,
        handler_conn: Option<xous::CID>,
        event_csr: utralib::CSR<u32>,
        event_susres: RegManager::<{utra::btevents::BTEVENTS_NUMREGS}>,
        power_csr: utralib::CSR<u32>,
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
        if xl.event_csr.rf(utra::btevents::EV_PENDING_RTC_INT) != 0 {
            if let Some(conn) = xl.handler_conn {
                xous::try_send_message(conn,
                    xous::Message::new_scalar(Opcode::EventRtcHappened.to_usize().unwrap(), 0, 0, 0, 0)).map(|_|()).unwrap();
            } else {
                log::error!("|handle_event_irq: RTC interrupt, but no connection for notification!")
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
            let crg_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::crg::HW_CRG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map CRG CSR range");
            let info_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::info::HW_INFO_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Info CSR range");
            let identifier_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::identifier_mem::HW_IDENTIFIER_MEM_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Identifier CSR range");
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
            let xadc_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng::HW_TRNG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Xadc CSR range"); // note that Xadc is "in" the TRNG because TRNG can override Xadc in hardware

            let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

            let mut xl = Llio {
                crg_csr: CSR::new(crg_csr.as_mut_ptr() as *mut u32),
                gpio_csr: CSR::new(gpio_base),
                gpio_susres: RegManager::new(gpio_base),
                info_csr: CSR::new(info_csr.as_mut_ptr() as *mut u32),
                identifier_csr: CSR::new(identifier_csr.as_mut_ptr() as *mut u32),
                handler_conn: Some(handler_conn), // connection for messages from IRQ handler
                event_csr: CSR::new(event_csr.as_mut_ptr() as *mut u32),
                event_susres: RegManager::new(event_csr.as_mut_ptr() as *mut u32),
                power_csr: CSR::new(power_csr.as_mut_ptr() as *mut u32),
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
            xl.gpio_susres.push(RegOrField::Reg(utra::gpio::USBDISABLE), None);

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
        pub fn suspend(&mut self) {
            self.uartmux_cache = self.gpio_csr.rf(UARTSEL_UARTSEL).into();
            self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 0); // set the kernel UART so we can catch KPs on boot

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
        pub fn rtc_int_ena(&mut self, ena: bool) {
            let value = if ena {1} else {0};
            self.event_csr.rmwf(utra::btevents::EV_ENABLE_RTC_INT, value);
        }
        pub fn com_int_ena(&mut self, ena: bool) {
            let value = if ena {1} else {0};
            self.event_csr.rmwf(utra::btevents::EV_ENABLE_COM_INT, value);
        }
        pub fn usb_int_ena(&mut self, ena: bool) {
            let value = if ena {1} else {0};
            self.power_csr.rmwf(utra::power::EV_PENDING_USB_ATTACH, value);
        }
        pub fn get_usb_disable(&self) -> bool {
            if self.gpio_csr.rf(utra::gpio::USBDISABLE_USBDISABLE) != 0 {
                true
            } else {
                false
            }
        }
        pub fn set_usb_disable(&mut self, state: bool) {
            if state {
                self.gpio_csr.wfo(utra::gpio::USBDISABLE_USBDISABLE, 1);
            } else {
                self.gpio_csr.wfo(utra::gpio::USBDISABLE_USBDISABLE, 0);
            }
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    use llio::api::*;

    #[derive(Copy, Clone, Debug)]
    pub struct Llio {
        usb_disable: bool,
    }
    pub fn log_init() -> *mut u32 { 0 as *mut u32 }

    impl Llio {
        pub fn new(_handler_conn: xous::CID, _gpio_base: *mut u32) -> Llio {
            Llio {
                usb_disable: false,
            }
        }
        pub fn suspend(&self) {}
        pub fn resume(&self) {}
        pub fn gpio_dout(&self, _d: u32) {}
        pub fn gpio_din(&self, ) -> u32 { 0xDEAD_BEEF }
        pub fn gpio_drive(&self, _d: u32) {}
        pub fn gpio_int_mask(&self, _d: u32) {}
        pub fn gpio_int_as_falling(&self, _d: u32) {}
        pub fn gpio_int_pending(&self, ) -> u32 { 0x0 }
        pub fn gpio_int_ena(&self, _d: u32) {}
        pub fn set_uart_mux(&self, _mux: UartType) {}
        pub fn get_info_dna(&self, ) ->  (usize, usize) { (0, 0) }
        pub fn get_info_git(&self, ) ->  (usize, usize) { (0, 0) }
        pub fn get_info_platform(&self, ) ->  (usize, usize) { (0, 0) }
        pub fn get_info_target(&self, ) ->  (usize, usize) { (0, 0) }
        pub fn power_audio(&self, _power_on: bool) {}
        pub fn power_crypto(&self, _power_on: bool) {}
        pub fn power_crypto_status(&self) -> (bool, bool, bool, bool) {
            (true, true, true, true)
        }
        pub fn power_self(&self, _power_on: bool) {}
        pub fn power_boost_mode(&self, _power_on: bool) {}
        pub fn ec_snoop_allow(&self, _power_on: bool) {}
        pub fn ec_reset(&self, ) {}
        pub fn ec_power_on(&self, ) {}
        pub fn self_destruct(&self, _code: u32) {}
        pub fn vibe(&self, pattern: VibePattern) {
            log::info!("Imagine your keyboard vibrating: {:?}", pattern);
        }


        pub fn xadc_vbus(&self) -> u16 {
            2 // some small but non-zero value to represent typical noise
        }
        pub fn xadc_vccint(&self) -> u16 {
            1296
        }
        pub fn xadc_vccaux(&self) -> u16 {
            2457
        }
        pub fn xadc_vccbram(&self) -> u16 {
            2450
        }
        pub fn xadc_usbn(&self) -> u16 {
            3
        }
        pub fn xadc_usbp(&self) -> u16 {
            4
        }
        pub fn xadc_temperature(&self) -> u16 {
            2463
        }
        pub fn xadc_gpio5(&self) -> u16 {
            0
        }
        pub fn xadc_gpio2(&self) -> u16 {
            0
        }

        pub fn rtc_int_ena(self, _ena: bool) {
        }
        pub fn com_int_ena(self, _ena: bool) {
        }
        pub fn usb_int_ena(self, _ena: bool) {
        }
        pub fn debug_powerdown(&mut self, _ena: bool) {
        }
        pub fn debug_wakeup(&mut self, _ena: bool) {
        }
        #[allow(dead_code)]
        pub fn activity_get_period(&mut self) -> u32 {
            12_000_000
        }
        pub fn wfi_override(&mut self, _override_: bool) {
        }

        pub fn get_usb_disable(&self) -> bool {
            self.usb_disable
        }
        pub fn set_usb_disable(&mut self, state: bool) {
            self.usb_disable = state;
        }
        pub fn tts_sleep_indicate(&mut self) {
        }
    }
}

fn i2c_thread(i2c_sid: xous::SID) {
    let xns = xous_names::XousNames::new().unwrap();

    let handler_conn = xous::connect(i2c_sid).expect("couldn't make handler connection for i2c");
    let mut i2c = i2c::I2cStateMachine::new(handler_conn);

    // register a suspend/resume listener
    let sr_cid = xous::connect(i2c_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(None, &xns, I2cOpcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut suspend_pending_token: Option<usize> = None;
    log::trace!("starting i2c main loop");
    loop {
        let mut msg = xous::receive_message(i2c_sid).unwrap();
        log::trace!("i2c message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(I2cOpcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                if !i2c.is_busy() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                } else {
                    // stash the token, and we'll do the suspend once the I2C transaction is done.
                    suspend_pending_token = Some(token);
                }
            }),
            Some(I2cOpcode::IrqI2cTxrxWriteDone) => msg_scalar_unpack!(msg, _, _, _, _, {
                if let Some(token) = suspend_pending_token.take() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                }
                // I2C state machine handler irq result
                i2c.report_write_done();
            }),
            Some(I2cOpcode::IrqI2cTxrxReadDone) => msg_scalar_unpack!(msg, _, _, _, _, {
                if let Some(token) = suspend_pending_token.take() {
                    i2c.suspend();
                    susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                    i2c.resume();
                }
                // I2C state machine handler irq result
                i2c.report_read_done();
            }),
            Some(I2cOpcode::I2cTxRx) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let i2c_txrx = buffer.to_original::<api::I2cTransaction, _>().unwrap();
                let status = i2c.initiate(i2c_txrx);
                buffer.replace(status).unwrap();
            },
            Some(I2cOpcode::I2cIsBusy) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let busy = if i2c.is_busy() {1} else {0};
                xous::return_scalar(msg.sender, busy as _).expect("couldn't return I2cIsBusy");
            }),
            Some(I2cOpcode::Quit) => {
                log::info!("Received quit opcode, exiting!");
                break;
            }
            None => {
                log::error!("Received unknown opcode: {:?}", msg);
            }
        }
    }
    xns.unregister_server(i2c_sid).unwrap();
    xous::destroy_server(i2c_sid).unwrap();
}


#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Llio;

    // very early on map in the GPIO base so we can have the right logging enabled
    let gpio_base = crate::implementation::log_init();

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // connections expected:
    // - codec
    // - GAM
    // - keyboard
    // - shellchat/sleep
    // - shellchat/environment
    // - shellchat/autoupdater
    // - spinor (for turning off wfi during writes)
    // - rootkeys (for reboots)
    // - oqc-test (for testing the vibe motor)
    // - net (for COM interrupt dispatch)
    // - pddb also allocates a connection, but then releases it, to read the DNA field.
    // We've migrated the I2C function out (which is arguably the most sensitive bit), so we can now set this more safely to unrestriced connection counts.
    let llio_sid = xns.register_name(api::SERVER_NAME_LLIO, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", llio_sid);

    // create the I2C handler thread
    // - codec
    // - rtc
    // - shellchat
    // I2C can be used to set time, which can have security implications; we are more strict on counting who can have access to this resource.
    let i2c_sid = xns.register_name(api::SERVER_NAME_I2C, Some(3)).expect("can't register I2C thread");
    log::trace!("registered I2C thread with NS -- {:?}", i2c_sid);
    let _ = thread::spawn({
        let i2c_sid = i2c_sid.clone();
        move || {
            i2c_thread(i2c_sid);
        }
    });
    log::trace!("spawning RTC server");
    // expected connections:
    // - status (for setting time)
    // - shellchat (for testing)
    // - rootkeys (for coordinating self-reboot)
    let rtc_sid = xns.register_name(api::SERVER_NAME_RTC, Some(3)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", rtc_sid);
    let _ = thread::spawn({
        let rtc_sid = rtc_sid.clone();
        move || {
            crate::rtc::rtc_server(rtc_sid);
        }
    });
    let rtc_conn = xous::connect(rtc_sid).unwrap();

    // Create a new llio object
    let handler_conn = xous::connect(llio_sid).expect("can't create IRQ handler connection");
    let mut llio = Llio::new(handler_conn, gpio_base);
    llio.ec_power_on(); // ensure this is set correctly; if we're on, we always want the EC on.

    if cfg!(feature = "wfi_off") {
        log::warn!("WFI is overridden at boot -- automatic power savings is OFF!");
        llio.wfi_override(true);
    }

    // register a suspend/resume listener
    let sr_cid = xous::connect(llio_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");
    let mut latest_activity = 0;

    let mut usb_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut com_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut rtc_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut gpio_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];

    let mut lockstatus_force_update = true; // some state to track if we've been through a susupend/resume, to help out the status thread with its UX update after a restart-from-cold

    log::trace!("starting main loop");
    loop {
        let mut msg = xous::receive_message(llio_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                llio.suspend();
                #[cfg(feature="tts")]
                llio.tts_sleep_indicate(); // this happens after the suspend call because we don't want the sleep indicator to be restored on resume
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                llio.resume();
                #[cfg(feature="tts")]
                llio.vibe(VibePattern::Double);
                lockstatus_force_update = true; // notify the status bar that yes, it does need to redraw the lock status, even if the value hasn't changed since the last read
            }),
            Some(Opcode::CrgMode) => msg_scalar_unpack!(msg, _mode, _, _, _, {
                todo!("CrgMode opcode not yet implemented.");
            }),
            Some(Opcode::GpioDataOut) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_dout(d as u32);
            }),
            Some(Opcode::GpioDataIn) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.gpio_din() as usize).expect("couldn't return gpio data in");
            }),
            Some(Opcode::GpioDataDrive) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_drive(d as u32);
            }),
            Some(Opcode::GpioIntMask) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_mask(d as u32);
            }),
            Some(Opcode::GpioIntAsFalling) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_as_falling(d as u32);
            }),
            Some(Opcode::GpioIntPending) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.gpio_int_pending() as usize).expect("couldn't return gpio pending vector");
            }),
            Some(Opcode::GpioIntEna) => msg_scalar_unpack!(msg, d, _, _, _, {
                llio.gpio_int_ena(d as u32);
            }),
            Some(Opcode::DebugPowerdown) => msg_scalar_unpack!(msg, arg, _, _, _, {
                let ena = if arg == 0 {false} else {true};
                llio.debug_powerdown(ena);
            }),
            Some(Opcode::DebugWakeup) => msg_scalar_unpack!(msg, arg, _, _, _, {
                let ena = if arg == 0 {false} else {true};
                llio.debug_wakeup(ena);
            }),
            Some(Opcode::UartMux) => msg_scalar_unpack!(msg, mux, _, _, _, {
                llio.set_uart_mux(mux.into());
            }),
            Some(Opcode::InfoDna) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_dna();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return DNA");
            }),
            Some(Opcode::InfoGit) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_git();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Git");
            }),
            Some(Opcode::InfoPlatform) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_platform();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Platform");
            }),
            Some(Opcode::InfoTarget) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (val1, val2) = llio.get_info_target();
                xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Target");
            }),
            Some(Opcode::PowerAudio) => msg_blocking_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_audio(false);
                } else {
                    llio.power_audio(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm audio power was set");
            }),
            Some(Opcode::PowerCrypto) => msg_blocking_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_crypto(false);
                } else {
                    llio.power_crypto(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm crypto power was set");
            }),
            Some(Opcode::WfiOverride) => msg_blocking_scalar_unpack!(msg, override_, _, _, _, {
                if override_ == 0 {
                    llio.wfi_override(false);
                } else {
                    llio.wfi_override(true);
                }
                xous::return_scalar(msg.sender, 0).expect("couldn't confirm wfi override was updated");
            }),
            Some(Opcode::PowerCryptoStatus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let (_, sha, engine, force) = llio.power_crypto_status();
                let mut ret = 0;
                if sha { ret |= 1 };
                if engine { ret |= 2 };
                if force { ret |= 4 };
                xous::return_scalar(msg.sender, ret).expect("couldn't return crypto unit power status");
            }),
            Some(Opcode::PowerSelf) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_self(false);
                } else {
                    llio.power_self(true);
                }
            }),
            Some(Opcode::PowerBoostMode) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.power_boost_mode(false);
                } else {
                    llio.power_boost_mode(true);
                }
            }),
            Some(Opcode::EcSnoopAllow) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                if power_on == 0 {
                    llio.ec_snoop_allow(false);
                } else {
                    llio.ec_snoop_allow(true);
                }
            }),
            Some(Opcode::EcReset) => msg_scalar_unpack!(msg, _, _, _, _, {
                llio.ec_reset();
            }),
            Some(Opcode::EcPowerOn) => msg_scalar_unpack!(msg, _, _, _, _, {
                llio.ec_power_on();
            }),
            Some(Opcode::SelfDestruct) => msg_scalar_unpack!(msg, code, _, _, _, {
                llio.self_destruct(code as u32);
            }),
            Some(Opcode::Vibe) => msg_scalar_unpack!(msg, pattern, _, _, _, {
                llio.vibe(pattern.into());
            }),
            Some(Opcode::AdcVbus) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vbus() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccInt) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccint() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccAux) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccaux() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcVccBram) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_vccbram() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcUsbN) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_usbn() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcUsbP) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_usbp() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcTemperature) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_temperature() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcGpio5) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_gpio5() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::AdcGpio2) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                xous::return_scalar(msg.sender, llio.xadc_gpio2() as _).expect("couldn't return Xadc");
            }),
            Some(Opcode::EventUsbAttachSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut usb_cb_conns);
            }
            Some(Opcode::EventComSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut com_cb_conns);
            }
            Some(Opcode::EventRtcSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut rtc_cb_conns);
            }
            Some(Opcode::GpioIntSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut gpio_cb_conns);
            }
            Some(Opcode::EventComEnable) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena == 0 {
                    llio.com_int_ena(false);
                } else {
                    llio.com_int_ena(true);
                }
            }),
            Some(Opcode::EventRtcEnable) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena == 0 {
                    llio.rtc_int_ena(false);
                } else {
                    llio.rtc_int_ena(true);
                }
            }),
            Some(Opcode::EventUsbAttachEnable) => msg_scalar_unpack!(msg, ena, _, _, _, {
                if ena == 0 {
                    llio.usb_int_ena(false);
                } else {
                    llio.usb_int_ena(true);
                }
            }),
            Some(Opcode::EventComHappened) => {
                send_event(&com_cb_conns, 0);
            },
            Some(Opcode::EventRtcHappened) => {
                send_event(&rtc_cb_conns, 0);
            },
            Some(Opcode::EventUsbHappened) => {
                send_event(&usb_cb_conns, 0);
            },
            Some(Opcode::GpioIntHappened) => msg_scalar_unpack!(msg, channel, _, _, _, {
                send_event(&gpio_cb_conns, channel as usize);
            }),
            Some(Opcode::EventActivityHappened) => msg_scalar_unpack!(msg, activity, _, _, _, {
                log::debug!("activity: {}", activity);
                latest_activity = activity as u32;
            }),
            Some(Opcode::GetActivity) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                #[cfg(any(target_os = "none", target_os = "xous"))]
                {
                    let period = llio.activity_get_period() as u32;
                    // log::debug!("activity/period: {}/{}, {:.2}%", latest_activity, period, (latest_activity as f32 / period as f32) * 100.0);
                    xous::return_scalar2(msg.sender, latest_activity as usize, period as usize).expect("couldn't return activity");
                }
                #[cfg(not(any(target_os = "none", target_os = "xous")))] // fake an activity
                {
                    let period = 12_000;
                    xous::return_scalar2(msg.sender, latest_activity as usize, period as usize).expect("couldn't return activity");
                    latest_activity += period / 20;
                    latest_activity %= period;
                }
            }),
            Some(Opcode::DebugUsbOp) => msg_blocking_scalar_unpack!(msg, update_req, new_state, _, _, {
                if update_req != 0 {
                    // if new_state is true (not 0), then try to lock the USB port
                    // if false, try to unlock the USB port
                    if new_state != 0 {
                        llio.set_usb_disable(true);
                    } else {
                        llio.set_usb_disable(false);
                    }
                }
                // at this point, *read back* the new state -- don't assume it "took". The readback is always based on
                // a real hardware value and not the requested value. for now, always false.
                let is_locked = if llio.get_usb_disable() {
                    1
                } else {
                    0
                };

                // this is a performance optimization. we could always redraw the status, but, instead we only redraw when
                // the status has changed. However, there is an edge case: on a resume from suspend, the status needs a redraw,
                // even if nothing has changed. Thus, we have this separate boolean we send back to force an update in the
                // case that we have just come out of a suspend.
                let force_update = if lockstatus_force_update {
                    1
                } else {
                    0
                };
                xous::return_scalar2(msg.sender, is_locked, force_update).expect("couldn't return status");
                lockstatus_force_update = false;
            }),
            Some(Opcode::DateTime) => {
                let alloc = DateTime::default();
                let mut buf = Buffer::into_buf(alloc).expect("couldn't transform to IPC memory");
                buf.lend_mut(rtc_conn, RtcOpcode::RequestDateTimeBlocking.to_u32().unwrap()).expect("RTC blocking get failed");
                let dt = buf.to_original::<DateTime, _>().expect("couldn't revert IPC memory");
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                buffer.replace(dt).unwrap();
            }
            Some(Opcode::Quit) => {
                log::info!("Received quit opcode, exiting.");
                let dropconn = xous::connect(i2c_sid).unwrap();
                xous::send_message(dropconn,
                    xous::Message::new_scalar(I2cOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
                unsafe{xous::disconnect(dropconn).unwrap();}

                let dropconn = xous::connect(i2c_sid).unwrap();
                xous::send_message(dropconn,
                    xous::Message::new_scalar(RtcOpcode::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
                unsafe{xous::disconnect(dropconn).unwrap();}
                break;
            }
            None => {
                log::error!("couldn't convert opcode: {:?}", msg);
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    unhook(&mut com_cb_conns);
    unhook(&mut rtc_cb_conns);
    unhook(&mut usb_cb_conns);
    unhook(&mut gpio_cb_conns);
    xns.unregister_server(llio_sid).unwrap();
    xous::destroy_server(llio_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = Some(ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
    });
    let mut found = false;
    for entry in cb_conns.iter_mut() {
        if entry.is_none() {
            *entry = cb_dat;
            found = true;
            break;
        }
    }
    if !found {
        log::error!("ran out of space registering callback");
    }
}
fn unhook(cb_conns: &mut [Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter_mut() {
        if let Some(scb) = entry {
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_blocking_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(scb.server_to_cb_cid).unwrap();}
        }
        *entry = None;
    }
}
fn send_event(cb_conns: &[Option<ScalarCallback>; 32], which: usize) {
    for entry in cb_conns.iter() {
        if let Some(scb) = entry {
            // note that the "which" argument is only used for GPIO events, to indicate which pin had the event
            match xous::try_send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(EventCallback::Event.to_usize().unwrap(),
                   scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, which, 0)
            ) {
                Ok(_) => {},
                Err(e) => {
                    match e {
                        xous::Error::ServerQueueFull => {
                            // this triggers if an interrupt storm happens. This could be perfectly natural and/or
                            // "expected", and the "best" behavior is probably to drop the events, but leave a warning.
                            // Examples of this would be a ping flood overwhelming the network stack.
                            log::warn!("Attempted to send event, but destination queue is full. Event was dropped: {:?}", scb);
                        }
                        xous::Error::ServerNotFound => {
                            log::warn!("Event callback subscriber has died. Event was dropped: {:?}", scb);
                        }
                        _ => {
                            log::error!("Callback error {:?}: {:?}", e, scb);
                        }
                    }
                }
            }
        };
    }
}
