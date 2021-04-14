#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;
mod i2c;
use i2c::*;

use log::{error, info};

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};

#[cfg(target_os = "none")]
mod implementation {
    use crate::api::*;
    use log::{error, info};
    use utralib::generated::*;
    use num_traits::ToPrimitive;

    #[allow(dead_code)]
    pub struct Llio {
        reboot_csr: utralib::CSR<u32>,
        crg_csr: utralib::CSR<u32>,
        gpio_csr: utralib::CSR<u32>,
        info_csr: utralib::CSR<u32>,
        identifier_csr: utralib::CSR<u32>,
        i2c_csr: utralib::CSR<u32>,
        handler_conn: Option<xous::CID>,
        event_csr: utralib::CSR<u32>,
        power_csr: utralib::CSR<u32>,
        seed_csr: utralib::CSR<u32>,
        xadc_csr: utralib::CSR<u32>,  // be careful with this as XADC is shared with TRNG
        ticktimer: ticktimer_server::Ticktimer,
        destruct_armed: bool,
    }

    fn handle_event_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        // just clear the pending request for now and return
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
        if let Some(conn) = xl.handler_conn {
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::EventUsbHappened.to_usize().unwrap(),
                    0, 0, 0, 0)).map(|_|()).unwrap();
        } else {
            log::error!("|handle_event_irq: USB interrupt, but no connection for notification!")
        }
        xl.power_csr
            .wo(utra::power::EV_PENDING, xl.power_csr.r(utra::power::EV_PENDING));
    }
    // ASSUME: we are only ever handling txrx done interrupts. If implementing ARB interrupts, this needs to be refactored to read the source and dispatch accordingly.
    fn handle_i2c_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        if let Some(conn) = xl.handler_conn {
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::IrqI2cTxrxDone.to_usize().unwrap(), 0, 0, 0, 0)).map(|_| ()).unwrap();
        } else {
            log::error!("|handle_i2c_irq: TXRX done interrupt, but no connection for notification!");
        }
        xl.i2c_csr
            .wo(utra::i2c::EV_PENDING, xl.i2c_csr.r(utra::i2c::EV_PENDING));
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
        gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 = kernel, 1 = log, 2 = app_uart

        gpio_base.as_mut_ptr() as *mut u32
    }

    impl Llio {
        pub fn get_i2c_base(&self) -> *mut u32 { self.i2c_csr.base }

        pub fn new(handler_conn: xous::CID, gpio_base: *mut u32) -> Llio {
            let reboot_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::reboot::HW_REBOOT_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Reboot CSR range");
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
            let i2c_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::i2c::HW_I2C_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map I2C CSR range");
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
            let seed_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::seed::HW_SEED_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Seed CSR range");
            let xadc_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng::HW_TRNG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Xadc CSR range"); // note that Xadc is "in" the TRNG because TRNG can override Xadc in hardware

            let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

            let mut xl = Llio {
                reboot_csr: CSR::new(reboot_csr.as_mut_ptr() as *mut u32),
                crg_csr: CSR::new(crg_csr.as_mut_ptr() as *mut u32),
                gpio_csr: CSR::new(gpio_base),
                info_csr: CSR::new(info_csr.as_mut_ptr() as *mut u32),
                identifier_csr: CSR::new(identifier_csr.as_mut_ptr() as *mut u32),
                i2c_csr: CSR::new(i2c_csr.as_mut_ptr() as *mut u32),
                handler_conn: Some(handler_conn), // connection for messages from IRQ handler
                event_csr: CSR::new(event_csr.as_mut_ptr() as *mut u32),
                power_csr: CSR::new(power_csr.as_mut_ptr() as *mut u32),
                seed_csr: CSR::new(seed_csr.as_mut_ptr() as *mut u32),
                xadc_csr: CSR::new(xadc_csr.as_mut_ptr() as *mut u32),
                ticktimer,
                destruct_armed: false,
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

            // disable interrupt, just in case it's enabled from e.g. a warm boot
            xl.i2c_csr.wfo(utra::i2c::EV_ENABLE_TXRX_DONE, 0);
            xous::claim_interrupt(
                utra::i2c::I2C_IRQ,
                handle_i2c_irq,
                (&mut xl) as *mut Llio as *mut usize,
            )
            .expect("couldn't claim I2C irq");

            // initialize i2c clocks
            // set the prescale assuming 100MHz cpu operation: 100MHz / ( 5 * 100kHz ) - 1 = 199
            let clkcode = (utralib::LITEX_CONFIG_CLOCK_FREQUENCY as u32) / (5 * 100_000) - 1;
            xl.i2c_csr.wfo(utra::i2c::PRESCALE_PRESCALE, clkcode & 0xFFFF);
            // enable the block
            xl.i2c_csr.rmwf(utra::i2c::CONTROL_EN, 1);
            // clear any interrupts pending, just in case something went pear-shaped during initialization
            xl.i2c_csr.wo(utra::i2c::EV_PENDING, xl.i2c_csr.r(utra::i2c::EV_PENDING));
            // now enable interrupts
            xl.i2c_csr.wfo(utra::i2c::EV_ENABLE_TXRX_DONE, 1);

            xl
        }

        pub fn reboot(&mut self, reboot_soc: bool) {
            if reboot_soc {
                self.reboot_csr.wfo(utra::reboot::SOC_RESET_SOC_RESET, 0xAC);
            } else {
                self.reboot_csr.wfo(utra::reboot::CPU_RESET_CPU_RESET, 1);
            }
        }
        pub fn set_reboot_vector(&mut self, vector: u32) {
            self.reboot_csr.wfo(utra::reboot::ADDR_ADDR, vector);
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
                UartType::Kernel => self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 0),
                UartType::Log => self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1),
                UartType::Application => self.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 2),
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
        pub fn get_info_seed(&self) -> (usize, usize) {
            (self.seed_csr.r(utra::seed::SEED0) as usize, self.info_csr.r(utra::seed::SEED1) as usize)
        }

        pub fn power_audio(&mut self, power_on: bool) {
            if power_on {
                self.power_csr.rmwf(utra::power::POWER_AUDIO, 1);
            } else {
                self.power_csr.rmwf(utra::power::POWER_AUDIO, 0);
            }
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
        pub fn power_boost_mode(&mut self, power_on: bool) {
            if power_on {
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
            self.power_csr.rmwf(utra::power::POWER_RESET_EC, 1);
            self.ticktimer.sleep_ms(20).unwrap();
            self.power_csr.rmwf(utra::power::POWER_RESET_EC, 0);
        }
        pub fn ec_power_on(&mut self) {
            self.power_csr.rmwf(utra::power::POWER_UP5K_ON, 1);
        }
        pub fn self_destruct(&mut self, code: u32) {
            if self.destruct_armed && code == 0x3141_5926 {
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
                    self.ticktimer.sleep_ms(250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                },
                VibePattern::Long => {
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    self.ticktimer.sleep_ms(1000).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                },
                VibePattern::Double => {
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    self.ticktimer.sleep_ms(250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                    self.ticktimer.sleep_ms(250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    self.ticktimer.sleep_ms(250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                },
            }
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
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use llio::api::*;
    use log::{error, info};

    #[derive(Copy, Clone, Debug)]
    pub struct Llio {
    }
    pub fn log_init() -> *mut u32 { 0 as *mut u32 }

    impl Llio {
        pub fn new(_handler_conn: xous::CID, _gpio_base: *mut u32) -> Llio {
            Llio {
            }
        }
        pub fn get_i2c_base(&self) -> *mut u32 { 0 as *mut u32 }

        pub fn reboot(&self, _reboot_soc: bool) {}
        pub fn set_reboot_vector(&self, _vector: u32) {}
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
        pub fn get_info_seed(&self, ) ->  (usize, usize) { (0, 0) }
        pub fn power_audio(&self, _power_on: bool) {}
        pub fn power_self(&self, _power_on: bool) {}
        pub fn power_boost_mode(&self, _power_on: bool) {}
        pub fn ec_snoop_allow(&self, _power_on: bool) {}
        pub fn ec_reset(&self, ) {}
        pub fn ec_power_on(&self, ) {}
        pub fn self_destruct(&self, _code: u32) {}
        pub fn vibe(&self, _pattern: VibePattern) {}


        pub fn xadc_vbus(&self) -> u16 {
            0
        }
        pub fn xadc_vccint(&self) -> u16 {
            0
        }
        pub fn xadc_vccaux(&self) -> u16 {
            0
        }
        pub fn xadc_vccbram(&self) -> u16 {
            0
        }
        pub fn xadc_usbn(&self) -> u16 {
            0
        }
        pub fn xadc_usbp(&self) -> u16 {
            0
        }
        pub fn xadc_temperature(&self) -> u16 {
            0
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
    }
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
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let llio_sid = xns.register_name(api::SERVER_NAME_LLIO).expect("can't register server");
    log::trace!("registered with NS -- {:?}", llio_sid);

    // Create a new llio object
    let handler_conn = xous::connect(llio_sid).expect("can't create IRQ handler connection");
    let mut llio = Llio::new(handler_conn, gpio_base);

    // ticktimer is a well-known server
    let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
    // create an i2c state machine handler
    let mut i2c_machine = I2cStateMachine::new(ticktimer, llio.get_i2c_base());

    let mut usb_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut com_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut rtc_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];
    let mut gpio_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];

    log::trace!("starting main loop");
    let mut reboot_requested: bool = false;
    loop {
        let mut msg = xous::receive_message(llio_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        if reboot_requested {
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::RebootCpuConfirm) => {
                    llio.reboot(false);
                }
                Some(Opcode::RebootSocConfirm) => {
                    llio.reboot(true);
                }
                _ => reboot_requested = false,
            }
        } else {
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::RebootRequest) => {
                    reboot_requested = true;
                },
                Some(Opcode::RebootCpuConfirm) => {
                    info!("RebootCpuConfirm, but no prior Request. Ignoring.");
                },
                Some(Opcode::RebootSocConfirm) => {
                    info!("RebootSocConfirm, but no prior Request. Ignoring.");
                },
                Some(Opcode::RebootVector) =>  msg_scalar_unpack!(msg, vector, _, _, _, {
                    llio.set_reboot_vector(vector as u32);
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
                Some(Opcode::InfoSeed) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    let (val1, val2) = llio.get_info_seed();
                    xous::return_scalar2(msg.sender, val1, val2).expect("couldn't return Seed");
                }),
                Some(Opcode::PowerAudio) => msg_scalar_unpack!(msg, power_on, _, _, _, {
                    if power_on == 0 {
                        llio.power_audio(false);
                    } else {
                        llio.power_audio(true);
                    }
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
                Some(Opcode::IrqI2cTxrxDone) => msg_scalar_unpack!(msg, _, _, _, _, {
                    // I2C state machine handler irq received
                    i2c_machine.handler();
                }),
                Some(Opcode::I2cTxRx) => {
                    let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    let i2c_txrx = buffer.to_original::<llio::api::I2cTransaction, _>().unwrap();
                    let status = i2c_machine.initiate(i2c_txrx);
                    buffer.replace(status).unwrap();
                }
                Some(Opcode::I2cIsBusy) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                    let busy = if i2c_machine.is_busy() {1} else {0};
                    xous::return_scalar(msg.sender, busy as _).expect("couldn't return I2cIsBusy");
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
                None => {
                    error!("couldn't convert opcode");
                    break;
                }
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
    xous::terminate_process(); loop {}
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
        error!("ran out of space registering callback");
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
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(EventCallback::Event.to_usize().unwrap(),
                   scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, which, 0)
            ).unwrap();
        };
    }
}