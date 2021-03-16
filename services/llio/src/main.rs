#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use llio::api::*;
mod i2c;
use i2c::*;

use core::convert::TryFrom;
use core::pin::Pin;
use rkyv::archived_value;
use rkyv::Unarchive;

use log::{error, info};

#[cfg(target_os = "none")]
mod implementation {
    use llio::api::*;
    use log::{error, info};
    use utralib::generated::*;

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
        ticktimer_conn: xous::CID,
        destruct_armed: bool,
    }

    fn handle_event_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        // just clear the pending request for now and return
        xl.event_csr
            .wo(utra::btevents::EV_PENDING, xl.event_csr.r(utra::btevents::EV_PENDING));
    }
    fn handle_gpio_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        // just clear the pending request for now and return
        xl.gpio_csr
            .wo(utra::gpio::EV_PENDING, xl.gpio_csr.r(utra::gpio::EV_PENDING));
    }
    // ASSUME: we are only ever handling txrx done interrupts. If implementing ARB interrupts, this needs to be refactored to read the source and dispatch accordingly.
    fn handle_i2c_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        if let Some(conn) = xl.handler_conn {
            xous::try_send_message(conn, Opcode::IrqI2cTxrxDone.into()).map(|_| ()).unwrap();
        } else {
            log::error!("LLIO|handle_i2c_irq: TXRX done interrupt, but no connection for notification!");
        }
        xl.i2c_csr
            .wo(utra::i2c::EV_PENDING, xl.i2c_csr.r(utra::i2c::EV_PENDING));
    }

    impl Llio {
        pub fn get_i2c_base(&self) -> *mut u32 { self.i2c_csr.base }

        pub fn new(handler_conn: xous::CID) -> Llio {
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
            let gpio_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map GPIO CSR range");
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

            let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
            let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

            let mut xl = Llio {
                reboot_csr: CSR::new(reboot_csr.as_mut_ptr() as *mut u32),
                crg_csr: CSR::new(crg_csr.as_mut_ptr() as *mut u32),
                gpio_csr: CSR::new(gpio_csr.as_mut_ptr() as *mut u32),
                info_csr: CSR::new(info_csr.as_mut_ptr() as *mut u32),
                identifier_csr: CSR::new(identifier_csr.as_mut_ptr() as *mut u32),
                i2c_csr: CSR::new(i2c_csr.as_mut_ptr() as *mut u32),
                handler_conn: Some(handler_conn), // connection for messages from IRQ handler
                event_csr: CSR::new(event_csr.as_mut_ptr() as *mut u32),
                power_csr: CSR::new(power_csr.as_mut_ptr() as *mut u32),
                seed_csr: CSR::new(seed_csr.as_mut_ptr() as *mut u32),
                xadc_csr: CSR::new(xadc_csr.as_mut_ptr() as *mut u32),
                ticktimer_conn,
                destruct_armed: false,
            };
            // setup the initial logging output
            xl.gpio_csr.wfo(utra::gpio::UARTSEL_UARTSEL, 1); // 0 = kernel, 1 = log, 2 = app_uart

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
                _ => info!("LLIO: invalid UART type specified for mux, doing nothing."),
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
                info!("LLIO: setting self-power state to on");
                self.power_csr.rmwf(utra::power::POWER_SELF, 1);
            } else {
                info!("LLIO: setting self-power state to OFF");
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
            ticktimer_server::sleep_ms(self.ticktimer_conn, 100).unwrap();
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
                error!("LLIO: self destruct attempted, but incorrect code sequence presented.");
            }
        }
        pub fn vibe(&mut self, pattern: VibePattern) {
            match pattern {
                VibePattern::Short => {
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    ticktimer_server::sleep_ms(self.ticktimer_conn, 250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                },
                VibePattern::Long => {
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    ticktimer_server::sleep_ms(self.ticktimer_conn, 1000).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                },
                VibePattern::Double => {
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    ticktimer_server::sleep_ms(self.ticktimer_conn, 250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 0);
                    ticktimer_server::sleep_ms(self.ticktimer_conn, 250).unwrap();
                    self.power_csr.wfo(utra::power::VIBE_VIBE, 1);
                    ticktimer_server::sleep_ms(self.ticktimer_conn, 250).unwrap();
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
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use crate::api::*;
    use log::{error, info};

    pub struct Llio {
    }

    impl Llio {
        pub fn new() -> Llio {
            Llio {
            }
        }

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
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    let debug1 = false;
    use crate::implementation::Llio;

    log_server::init_wait().unwrap();
    info!("LLIO: my PID is {}", xous::process::id());

    let llio_sid = xous_names::register_name(xous::names::SERVER_NAME_LLIO).expect("LLIO: can't register server");
    if debug1{info!("LLIO: registered with NS -- {:?}", llio_sid);}

    // Create a new llio object
    let handler_conn = xous::connect(llio_sid).expect("LLIO: can't create IRQ handler connection");
    let mut llio = Llio::new(handler_conn);

    // ticktimer is a well-known server
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();
    // create an i2c state machine handler
    let mut i2c_machine = I2cStateMachine::new(ticktimer_conn, llio.get_i2c_base());

    if debug1{info!("LLIO: starting main loop");}
    let mut reboot_requested: bool = false;
    loop {
        let envelope = xous::receive_message(llio_sid).unwrap();
        if debug1{info!("LLIO: Message: {:?}", envelope)};
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            // info!("LLIO: Opcode: {:?}", opcode);
            // reset the reboot request if the very next opcode is not a confirm
            if reboot_requested {
                match opcode {
                    Opcode::RebootCpuConfirm => {
                        if reboot_requested {
                            llio.reboot(false);
                        }
                    },
                    Opcode::RebootSocConfirm => {
                        if reboot_requested {
                            llio.reboot(true);
                        }
                    },
                    _ => reboot_requested = false,
                }
            }
            match opcode {
                Opcode::RebootRequest => {
                    reboot_requested = true;
                },
                Opcode::RebootCpuConfirm => {
                    info!("LLIO: RebootCpuConfirm, but no prior Request. Ignoring.");
                },
                Opcode::RebootSocConfirm => {
                    info!("LLIO: RebootSocConfirm, but no prior Request. Ignoring.");
                },
                Opcode::RebootVector(vector) => {
                    llio.set_reboot_vector(vector);
                },
                Opcode::CrgMode(_mode) => {
                    todo!("LLIO: CrgMode opcode not yet implemented.");
                },
                Opcode::GpioDataOut(d) => {
                    llio.gpio_dout(d);
                },
                Opcode::GpioDataIn => {
                    xous::return_scalar(envelope.sender, llio.gpio_din() as usize).expect("LLIO: couldn't return gpio data in");
                },
                Opcode::GpioDataDrive(d) => {
                    llio.gpio_drive(d);
                },
                Opcode::GpioIntMask(d) => {
                    llio.gpio_int_mask(d);
                },
                Opcode::GpioIntAsFalling(d) => {
                    llio.gpio_int_as_falling(d);
                },
                Opcode::GpioIntPending => {
                    xous::return_scalar(envelope.sender, llio.gpio_int_pending() as usize).expect("LLIO: couldn't return gpio pending vector");
                },
                Opcode::GpioIntEna(d) => {
                    llio.gpio_int_ena(d);
                },
                Opcode::UartMux(mux) => {
                    llio.set_uart_mux(mux);
                },
                Opcode::InfoDna => {
                    let (val1, val2) = llio.get_info_dna();
                    xous::return_scalar2(envelope.sender, val1, val2).expect("LLIO: couldn't return DNA");
                },
                Opcode::InfoGit => {
                    let (val1, val2) = llio.get_info_git();
                    xous::return_scalar2(envelope.sender, val1, val2).expect("LLIO: couldn't return Git");
                },
                Opcode::InfoPlatform => {
                    let (val1, val2) = llio.get_info_platform();
                    xous::return_scalar2(envelope.sender, val1, val2).expect("LLIO: couldn't return Platform");
                },
                Opcode::InfoTarget => {
                    let (val1, val2) = llio.get_info_target();
                    xous::return_scalar2(envelope.sender, val1, val2).expect("LLIO: couldn't return Target");
                },
                Opcode::InfoSeed => {
                    let (val1, val2) = llio.get_info_seed();
                    xous::return_scalar2(envelope.sender, val1, val2).expect("LLIO: couldn't return Seed");
                },
                Opcode::PowerAudio(power_on) => {
                    llio.power_audio(power_on);
                },
                Opcode::PowerSelf(power_on) => {
                    llio.power_self(power_on);
                },
                Opcode::PowerBoostMode(power_on) => {
                    llio.power_boost_mode(power_on);
                },
                Opcode::EcSnoopAllow(allow) => {
                    llio.ec_snoop_allow(allow);
                },
                Opcode::EcReset => {
                    llio.ec_reset();
                },
                Opcode::EcPowerOn => {
                    llio.ec_power_on();
                },
                Opcode::SelfDestruct(code) => {
                    llio.self_destruct(code);
                },
                Opcode::Vibe(pattern) => {
                    llio.vibe(pattern);
                },
                Opcode::AdcVbus => {
                    xous::return_scalar(envelope.sender, llio.xadc_vbus() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcVccInt => {
                    xous::return_scalar(envelope.sender, llio.xadc_vccint() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcVccAux => {
                    xous::return_scalar(envelope.sender, llio.xadc_vccaux() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcVccBram => {
                    xous::return_scalar(envelope.sender, llio.xadc_vccbram() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcUsbN => {
                    xous::return_scalar(envelope.sender, llio.xadc_usbn() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcUsbP => {
                    xous::return_scalar(envelope.sender, llio.xadc_usbp() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcTemperature => {
                    xous::return_scalar(envelope.sender, llio.xadc_temperature() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcGpio5 => {
                    xous::return_scalar(envelope.sender, llio.xadc_gpio5() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::AdcGpio2 => {
                    xous::return_scalar(envelope.sender, llio.xadc_gpio2() as _).expect("LLIO: couldn't return Xadc");
                },
                Opcode::IrqI2cTxrxDone => {
                    // I2C state machine handler irq received
                    i2c_machine.handler();
                },
            _ => error!("LLIO: no handler for opcode"),
            }
        } else if let xous::Message::MutableBorrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<Opcode>::I2cTxRx(rkyv_i2c) => {
                    let mut i2c_txrx: I2cTransaction = rkyv_i2c.unarchive();

                    let status = i2c_machine.initiate(i2c_txrx);

                    i2c_txrx.status = status;
                    // pack our data back into the buffer to return
                    use rkyv::Write;
                    let mut writer = rkyv::ArchiveBuffer::new(buf);
                    writer.archive(&Opcode::I2cTxRx(i2c_txrx)).expect("LLIO: couldn't re-archive return value to I2cTxRx");
                },
                _ => panic!("LLIO: invalid MutableBorrow memory message")
            };
        } else if let xous::Message::Borrow(m) = &envelope.body {
            let buf = unsafe { xous::XousBuffer::from_memory_message(m) };
            let bytes = Pin::new(buf.as_ref());
            let value = unsafe {
                archived_value::<api::Opcode>(&bytes, m.id as usize)
            };
            match &*value {
                rkyv::Archived::<api::Opcode>::I2cSubscribe(registration) => {
                    let reg: xous::String::<64> = registration.unarchive();
                    let cid = xous_names::request_connection_blocking(reg.to_str()).expect("KBD: can't connect to requested listener for reporting events");
                    i2c_machine.register_listener(cid).expect("LLIO: probably ran out of slots for I2C event reporting");
                },
                _ => panic!("LLIO: invalid Borrow memory message"),
            };
        } else {
            error!("LLIO: couldn't convert opcode");
        }
    }
}
