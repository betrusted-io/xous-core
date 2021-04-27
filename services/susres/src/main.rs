#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::FromPrimitive;

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let sr = unsafe{ &mut *(arg as *mut SusResHw) };
        // dummy routine for now
        sr.csr.wfo(utra::susres::EV_PENDING_SOFT_INT, 1);
    }

    pub struct SusResHw {
        /// our CSR
        csr: utralib::CSR<u32>,
        /// memory region for the "clean suspend" marker
        marker: xous::MemoryRange,
        /// loader stack region -- this data is dirtied on every resume; claim it in this process so no others accidentally use it
        loader_stack: xous::MemoryRange,
        /// backing store for the ticktimer value
        time_backing: Option<[u32; 2]>,
    }
    impl SusResHw {
        pub fn new() -> Self {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::susres::HW_SUSRES_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map SusRes CSR range");

            let marker = xous::syscall::map_memory(
                xous::MemoryAddress::new(0x40FFE000), // this is a special, hard-coded location
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ).expect("couldn't map clean suspend page");
            let loader_stack = xous::syscall::map_memory(
                xous::MemoryAddress::new(0x40FFF000), // this is a special, hard-coded location
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ).expect("couldn't map clean suspend page");
            let mut sr = SusResHw {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                time_backing: None,
                marker,
                loader_stack,
            };

            // check that the marker has been zerod by map_memory
            // TODO

            xous::claim_interrupt(
                utra::susres::SUSRES_IRQ,
                handle_irq,
                (&mut sr) as *mut SusResHw as *mut usize,
            ).expect("couldn't claim IRQ");

            sr
        }
    }

}

#[cfg(not(target_os = "none"))]
mod implementation {
    pub struct SusResHw {
    }
    impl SusResHw {
        pub fn new() -> Self {
            SusResHw {}
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let susres_sid = xns.register_name(api::SERVER_NAME_SUSRES).expect("can't register server");
    log::trace!("registered with NS -- {:?}", susres_sid);

    let susres_hw = implementation::SusResHw::new();

    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        /*
        match FromPrimitive::from_usize(msg.body.id()) {
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }*/
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(susres_sid).unwrap();
    xous::destroy_server(susres_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
