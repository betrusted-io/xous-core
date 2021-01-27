#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use core::convert::TryFrom;

use log::{error, info};

use xous::CID;

#[cfg(target_os = "none")]
mod implementation {
    use crate::api::*;
    use log::{error, info};
    use utralib::generated::*;
    use xous::CID;

    use heapless::Vec;
    use heapless::consts::*;

    const STD_TIMEOUT: u32 = 100;

    pub struct Llio {
        reboot_csr: utralib::CSR<u32>,
        crg_csr: utralib::CSR<u32>,
        gpio_csr: utralib::CSR<u32>,
        info_csr: utralib::CSR<u32>,
        identifier_csr: utralib::CSR<u32>,
        i2c_csr: utralib::CSR<u32>,
        event_csr: utralib::CSR<u32>,
        power_csr: utralib::CSR<u32>,
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
    fn handle_i2c_irq(_irq_no: usize, arg: *mut usize) {
        let xl = unsafe { &mut *(arg as *mut Llio) };
        // just clear the pending request for now and return
        xl.i2c_csr
            .wo(utra::i2c::EV_PENDING, xl.i2c_csr.r(utra::i2c::EV_PENDING));
    }

    impl Llio {
        pub fn new() -> Llio {
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

            let mut xl = Llio {
                reboot_csr: CSR::new(reboot_csr.as_mut_ptr() as *mut u32),
                crg_csr: CSR::new(crg_csr.as_mut_ptr() as *mut u32),
                gpio_csr: CSR::new(gpio_csr.as_mut_ptr() as *mut u32),
                info_csr: CSR::new(info_csr.as_mut_ptr() as *mut u32),
                identifier_csr: CSR::new(identifier_csr.as_mut_ptr() as *mut u32),
                i2c_csr: CSR::new(i2c_csr.as_mut_ptr() as *mut u32),
                event_csr: CSR::new(event_csr.as_mut_ptr() as *mut u32),
                power_csr: CSR::new(power_csr.as_mut_ptr() as *mut u32),
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
                utra::i2c::I2C_IRQ,
                handle_i2c_irq,
                (&mut xl) as *mut Llio as *mut usize,
            )
            .expect("couldn't claim I2C irq");

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

        pub fn reboot(_reboot_soc: bool) {}
        pub fn set_rebot_vector(_vector: u32) {}
        pub fn gpio_dout(_d: u32) {}
        pub fn gpio_din() -> u32 { 0xDEAD_BEEF }
        pub fn gpio_drive(_d: u32) {}
        pub fn gpio_int_mask(_d: u32) {}
        pub fn gpio_int_as_falling(_d: u32) {}
        pub fn gpio_int_pending() -> u32 { 0x0 }
        pub fn gpio_int_ena(_d: u32) {}
        pub fn set_uart_mux(_mux: UartType) {}
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Llio;
    use heapless::Vec;
    use heapless::consts::*;

    log_server::init_wait().unwrap();

    let llio_sid = xous_names::register_name(xous::names::SERVER_NAME_LLIO).expect("LLIO: can't register server");
    info!("LLIO: registered with NS -- {:?}", llio_sid);

    // Create a new com object
    let mut llio = Llio::new();

    info!("LLIO: starting main loop");
    let mut reboot_requested: bool = false;
    loop {
        let envelope = xous::receive_message(llio_sid).unwrap();
        // info!("LLIO: Message: {:?}", envelope);
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
                Opcode::GpioIntSubscribe(_Registration) => {
                    todo!("LLIO: GPIO push interrupt events not yet implemented.");
                },
                Opcode::UartMux(mux) => {
                    llio.set_uart_mux(mux);
                },
            _ => error!("LLIO: no handler for opcode"),
            }
        } else {
            error!("LLIO: couldn't convert opcode");
        }
    }
}
