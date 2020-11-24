#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
#[macro_use]
mod debug;

mod api;
use api::Opcode;

use core::convert::TryFrom;

/*
use heapless::binary_heap::{BinaryHeap, Min};
use heapless::Vec;
use heapless::consts::*;
type U1280 = UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B1>, B0>, B0>, B0>, B0>, B0>, B0>, B0>, B0>;
*/

use log::{error, info};

use com_rs::*;

//#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;

    pub struct XousCom {
        csr: utralib::CSR<u32>,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let _xc = unsafe { &mut *(arg as *mut XousCom) };
        println!("COM IRQ");
    }

    impl XousCom {
        pub fn new() -> XousCom {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::com::HW_COM_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map COM CSR range");

            let mut xc = XousCom {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
            };

            xous::claim_interrupt(
                utra::com::COM_IRQ,
                handle_irq,
                (&mut xc) as *mut XousCom as *mut usize,
            )
            .expect("couldn't claim irq");
            xc
        }

        pub fn txrx(&mut self, tx: u16) -> u16 {
            self.csr.wfo(utra::com::TX_TX, tx as u32);  // transaction is automatically initiated on write
            // wait while transaction is in progress. A transaction takes about 80-100 CPU cycles;
            // not quite enough to be worth the overhead of an interrupt, so we just yield our time slice
            while self.csr.rf(utra::com::STATUS_TIP) == 1 {
                xous::yield_slice();
            }

            // grab the RX value and return it
            self.csr.rf(utra::com::RX_RX) as u16
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct XousCom {
    }

    impl XousCom {
        pub fn new() -> XousCom {
            XousCom {
            }
        }

        pub fn txrx(tx: u16) -> u16 {
            0xDEAD as u16
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::XousCom;

    println!("COM Init");
    log_server::init_wait().unwrap();

    let com_server =
        xous::create_server(b"com             ").expect("Couldn't create COM server");

    // Create a new com object
    let mut com = XousCom::new();

    loop {
        info!("COM: waiting for message");
        let envelope = xous::receive_message(com_server).unwrap();
        info!("COM: Message: {:?}", envelope);
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            info!("COM: Opcode: {:?}", opcode);
            match opcode {
                Opcode::PowerOffSoc => {
                    info!("COM: power off called");
                    com.txrx(ComState::POWER_OFF.verb);
                }
                _ => error!("unknown opcode"),
            }
        } else {
            error!("couldn't convert opcode");
        }
    }
}

// xous::wait_event()
