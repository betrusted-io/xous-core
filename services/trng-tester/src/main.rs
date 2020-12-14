#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::{error, info};

pub enum WhichMessible {
    One,
    Two,
}

pub const TRNG_BUFF_LEN: usize = 512*1024;

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use xous::MemoryRange;
    use crate::WhichMessible;

    pub struct Trng {
        server_csr: utralib::CSR<u32>,
        xadc_csr: utralib::CSR<u32>,
        messible_csr: utralib::CSR<u32>,
        messible2_csr: utralib::CSR<u32>,
        buffer: MemoryRange,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let trng = unsafe { &mut *(arg as *mut Trng) };
        // just clear the pending request, as this is used as a "wait" until request function
        trng.server_csr.wo(utra::trng_server::EV_PENDING, trng.server_csr.r(utra::trng_server::EV_PENDING));
    }

    impl Trng {
        pub fn new() -> Trng {
            let server_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng_server::HW_TRNG_SERVER_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG Server CSR range");

            let xadc_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng::HW_TRNG_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG xadc CSR range");

            let buff = xous::syscall::map_memory(
                xous::MemoryAddress::new(HW_SRAM_EXT_MEM + (1024 * 1024 * 8)), // fix this at a known physical address
                None,
                crate::TRNG_BUFF_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG comms buffer");

            let messible = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::messible::HW_MESSIBLE_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map messible");

            let messible2 = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::messible2::HW_MESSIBLE2_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map messible");

            let mut trng = Trng {
                server_csr: CSR::new(server_csr.as_mut_ptr() as *mut u32),
                xadc_csr: CSR::new(xadc_csr.as_mut_ptr() as *mut u32),
                buffer: buff,
                messible_csr: CSR::new(messible.as_mut_ptr() as *mut u32),
                messible2_csr: CSR::new(messible2.as_mut_ptr() as *mut u32),
            };

            xous::claim_interrupt(
                utra::trng_server::TRNG_SERVER_IRQ,
                handle_irq,
                (&mut trng) as *mut Trng as *mut usize,
            )
            .expect("couldn't claim irq");

            trng
        }

        pub fn init(&mut self) {
            self.server_csr.wo(utra::trng_server::CONTROL,
                self.server_csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                | self.server_csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                // | self.server_csr.ms(utra::trng_server::CONROL_RO_DIS, 1)  // disable the RO to characterize only the AV
            );
            // delay in microseconds for avalanche poweron after powersave
            self.server_csr.wfo(utra::trng_server::AV_CONFIG_POWERDELAY, 200_000);
        }

        pub fn messible_send(&mut self, which: WhichMessible, value: u8) {
            match which {
                WhichMessible::One => self.messible_csr.wfo(utra::messible::IN_IN, value as u32),
                WhichMessible::Two => self.messible_csr.wfo(utra::messible2::IN_IN, value as u32),
            }
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct Trng {
    }

    impl Trng {
        pub fn new() -> Trng {
            Trng {
            }
        }

        pub fn messible_send(&mut self, which: WhichMessible, value: u32) {
        }

        pub fn init(&mut self) {
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Trng;

    log_server::init_wait().unwrap();

    let log_server_id = xous::SID::from_bytes(b"xous-log-server ").unwrap();
    let log_conn = xous::connect(log_server_id).unwrap();

    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    // Create a new com object
    let mut trng = Trng::new();
    trng.init();

    info!("TRNG: starting service");
    let mut phase: u8 = 1;

    trng.messible_send(WhichMessible::One, phase);

    loop {
        ticktimer_server::sleep_ms(ticktimer_conn, 100).expect("couldn't sleep");

    }
}
