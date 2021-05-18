#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use api::{Opcode, SusResOps, Sha2Config};
use xous::{msg_scalar_unpack, msg_blocking_scalar_unpack};

use log::info;

use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use susres::{RegManager, RegOrField, SuspendResume};

    pub struct Engine512 {
        csr: utralib::CSR<u32>,
        fifo: xous::MemoryRange,
        susres_manager: RegManager::<{utra::sha512::SHA512_NUMREGS}>,
        in_progress: bool,
    }

    impl Engine512 {
        pub fn new() -> Engine512 {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::sha512::HW_SHA512_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Engine512 CSR range");
            let fifo = xous::syscall::map_memory(
                xous::MemoryAddress::new(utralib::HW_SHA512_MEM),
                None,
                utralib::HW_SHA512_MEM_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Engine512 CSR range");

            let mut engine512 = Engine512 {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
                fifo,
                in_progress: false,
            };

            engine512
        }

        pub fn suspend(&mut self) {
            self.susres_manager.suspend();
        }
        pub fn resume(&mut self) {
            self.susres_manager.resume();
        }

        pub fn reset(&mut self) {
            ///////////// TODO
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Engine512 {
    }

    impl Engine512 {
        pub fn new() -> Engine512 {
            Engine512 {
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
        pub fn reset(&self) {
        }
    }
}

static HASH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

fn susres_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let susres_sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();

    // register a suspend/resume listener
    let sr_cid = xous::connect(susres_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::SusResOps::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    log::trace!("starting Sha512 suspend/resume manager loop");
    loop {
        let mut msg = xous::receive_message(susres_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SusResOps::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                while HASH_IN_PROGRESS.load(Ordering::Relaxed) {
                    xous::yield_slice();
                }
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
            }),
            Some(SusResOps::Quit) => {
                log::info!("Received quit opcode, exiting!");
                break;
            }
            None => {
                log::error!("Received unknown opcode: {:?}", msg);
            }
        }
    }
    xns.unregister_server(susres_sid).unwrap();
    xous::destroy_server(susres_sid).unwrap();
}


#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Engine512;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let engine512_sid = xns.register_name(api::SERVER_NAME_SHA512).expect("can't register server");
    log::trace!("registered with NS -- {:?}", engine512_sid);

    let mut engine512 = Engine512::new();

    log::trace!("ready to accept requests");

    // handle suspend/resume with a separate thread, which monitors our in-progress state
    // we can't save hardware state of a hash, so the hash MUST finish before we can suspend.
    let susres_mgr_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = susres_mgr_sid.to_u32();
    xous::create_thread_4(susres_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize).expect("couldn't start susres handler thread");

    let mut client_id: Option<[u32; 3]> = None;
    let mut mode: Option<Sha2Config> = None;

    loop {
        let msg = xous::receive_message(engine512_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::AcquireExclusive) => msg_blocking_scalar_unpack!(msg, id0, id1, id2, flags, {
                if client_id.is_none() {
                    client_id = Some([id0 as u32, id1 as u32, id2 as u32]);
                    mode = Some(FromPrimitive::from_usize(flags).unwrap());
                    HASH_IN_PROGRESS.store(true, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1);
                } else {
                    xous::return_scalar(msg.sender, 0);
                }
            }),
            Some(Opcode::Reset) => msg_blocking_scalar_unpack!(msg, r_id0, r_id1, r_id2, _, {
                match client_id {
                    Some([id0, id1, id2]) => {
                        if id0 == r_id0 as u32 && id1 == r_id1 as u32 && id2 == r_id2 as u32 {
                            client_id = None;
                            mode = None;
                            engine512.reset();
                            xous::return_scalar(msg.sender, 1);
                        } else {
                            xous::return_scalar(msg.sender, 0);
                        }
                    }
                    _ => {
                        xous::return_scalar(msg.sender, 0);
                    }
                }
            }),
            Some(Opcode::Quit) => {
                log::info!("Received quit opcode, exiting!");
                break;
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    let quitconn = xous::connect(susres_mgr_sid).unwrap();
    xous::send_message(quitconn, xous::Message::new_scalar(SusResOps::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(quitconn);}

    xns.unregister_server(engine512_sid).unwrap();
    xous::destroy_server(engine512_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
