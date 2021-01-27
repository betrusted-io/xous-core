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
    use log::error;
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
    loop {
        let envelope = xous::receive_message(llio_sid).unwrap();
        // info!("LLIO: Message: {:?}", envelope);
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            // info!("LLIO: Opcode: {:?}", opcode);
            match opcode {
                _ => error!("unknown opcode"),
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}
