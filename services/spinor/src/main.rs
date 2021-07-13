#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::msg_blocking_scalar_unpack;

use core::sync::atomic::{AtomicBool, Ordering};


#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use log::info;
    use susres::{RegManager, RegOrField, SuspendResume};

    pub struct Spinor {
        csr: utralib::CSR<u32>,
        susres_manager: RegManager::<{utra::spinor::SPINOR_NUMREGS}>,
        ops_count: u32, // tracks total ops done; also serves as a delay to ensure that the WIP bit updates before being polled
    }

    impl Spinor {
        pub fn new() -> Spinor {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::spinor::HW_SPINOR_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map SPINOR CSR range");

            let mut spinor = Spinor {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
                ops_count: 0,
            };

            spinor
        }

        pub fn write_region(&mut self, wr: &mut WriteRegion) {
            if wr.start + wr.len > SPINOR_SIZE_BYTES {
                wr.result = Some(SpinorError::InvalidRequest);
                return;
            }
            
        }
        pub fn erase_region(&mut self, start_adr: u32, num_u8: u32) -> SpinorError {

        }

        pub fn suspend(&mut self) {
            self.susres_manager.suspend();
        }
        pub fn resume(&mut self) {
            self.susres_manager.resume();
        }

        fn flash_rdsr(&mut self, lock_reads: u32) -> u32 {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                | self.csr.ms(spinor::COMMAND_LOCK_READS, lock_reads)
                | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x05) // RDSR
                | self.csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
                | self.csr.ms(spinor::COMMAND_DATA_WORDS, 1)
                | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
            );
            self.ops_count += 1; // count number of ops performed; also delays polling of WIP to ensure the command machine has time to respond
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
            self.csr.r(utra::spinor::CMD_RBK_DATA)
        }

        fn flash_rdscur(&mut self) -> u32 {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
            self.csr.wo(utra::spinor::COMMAND,
                  self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
                | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
                | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x2B) // RDSCUR
                | self.csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
                | self.csr.ms(spinor::COMMAND_DATA_WORDS, 1)
                | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
            self.csr.r(utra::spinor::CMD_RBK_DATA)
        }

        fn flash_rdid(&mut self, offset: u32) -> u32 {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x9f)  // RDID
              | self.csr.ms(spinor::COMMAND_DUMMY_CYCLES, 4)
              | self.csr.ms(spinor::COMMAND_DATA_WORDS, offset) // 2 -> 0x3b3b8080, // 1 -> 0x8080c2c2
              | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
            self.csr.r(utra::spinor::CMD_RBK_DATA)
        }

        fn flash_wren(&mut self) {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x06)  // WREN
              | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        }

        fn flash_wrdi(&mut self) {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x04)  // WRDI
              | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        }

        fn flash_se4b(&mut self, sector_address: u32) {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, sector_address);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x21)  // SE4B
              | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
              | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        }

        fn flash_be4b(&mut self, block_address: u32) {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, block_address);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0xdc)  // BE4B
              | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
              | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        }

        fn flash_pp4b(&mut self, address: u32, data_bytes: u32) {
            self.csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, address);
            self.csr.wo(utra::spinor::COMMAND,
                self.csr.ms(spinor::COMMAND_EXEC_CMD, 1)
              | self.csr.ms(spinor::COMMAND_CMD_CODE, 0x12)  // PP4B
              | self.csr.ms(spinor::COMMAND_HAS_ARG, 1)
              | self.csr.ms(spinor::COMMAND_DATA_WORDS, data_bytes / 2)
              | self.csr.ms(spinor::COMMAND_LOCK_READS, 1)
            );
            self.ops_count += 1;
            while self.csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Spinor {
    }

    impl Spinor {
        pub fn new() -> Spinor {
            Spinor {
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
    }
}


static OP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static SUSPEND_FAILURE: AtomicBool = AtomicBool::new(false);
static SUSPEND_PENDING: AtomicBool = AtomicBool::new(false);

fn susres_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let susres_sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();

    // register a suspend/resume listener
    let sr_cid = xous::connect(susres_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::SusResOps::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    log::trace!("starting SPINOR suspend/resume manager loop");
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SusResOps::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                SUSPEND_PENDING.store(true, Ordering::Relaxed);
                while OP_IN_PROGRESS.load(Ordering::Relaxed) {
                    xous::yield_slice();
                }
                if susres.suspend_until_resume(token).expect("couldn't execute suspend/resume") == false {
                    SUSPEND_FAILURE.store(true, Ordering::Relaxed);
                } else {
                    SUSPEND_FAILURE.store(false, Ordering::Relaxed);
                }
                SUSPEND_PENDING.store(false, Ordering::Relaxed);
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
    use crate::implementation::Spinor;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    /*
        Very important to track who has access to the SPINOR server, and limit access. Access to this server is essential for persistent rootkits.
        Here is the list of servers allowed to access, and why:
          - shellchat (for testing ONLY, remove once done)
          - suspend/resume (for suspend locking/unlocking calls)
          - PDDB (not yet written)
          - keystore (not yet written)
    */
    let spinor_sid = xns.register_name(api::SERVER_NAME_SPINOR, Some(2)).expect("can't register server");
    log::trace!("registered with NS -- {:?}", spinor_sid);

    let mut spinor = Spinor::new();

    log::trace!("ready to accept requests");

    // handle suspend/resume with a separate thread, which monitors our in-progress state
    // we can't interrupt an erase or program operation, so the op MUST finish before we can suspend.
    let susres_mgr_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = susres_mgr_sid.to_u32();
    xous::create_thread_4(susres_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize).expect("couldn't start susres handler thread");

    let mut client_id: Option<[u32; 4]> = None;

    loop {
        let mut msg = xous::receive_message(spinor_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::AcquireExclusive) => msg_blocking_scalar_unpack!(msg, id0, id1, id2, id3, {
                if client_id.is_none() && !SUSPEND_PENDING.load(Ordering::Relaxed) {
                    client_id = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                    log::trace!("giving {:x?} an exclusive lock", client_id);
                    SUSPEND_FAILURE.store(false, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1).unwrap();
                } else {
                    xous::return_scalar(msg.sender, 0).unwrap();
                }
            }),
            Some(Opcode::ReleaseExclusive) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                client_id = None;
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::AcquireSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if client_id.is_none() {
                    SUSPEND_PENDING.store(true, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1).expect("couldn't ack AcquireSuspendLock");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't ack AcquireSuspendLock");
                }
            }),
            Some(Opcode::ReleaseSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                SUSPEND_PENDING.store(false, Ordering::Relaxed);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack ReleaseSuspendLock");
            }),
            Some(Opcode::EraseRegion) => msg_blocking_scalar_unpack!(msg, start_adr, num_u8, _, _, {
                let ret = spinor.erase_region(start_adr as u32, num_u8 as u32);
                xous::return_scalar(msg.sender, ret.to_usize().unwrap()).expect("couldn't return EraseRegion response");
            }),
            Some(Opcode::WriteRegion) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut wr = buffer.to_original::<WriteRegion, _>().unwrap();
                match client_id {
                    Some(id) => {
                        if wr.id == id {
                            spinor.write_region(&mut wr);
                        } else {
                            wr.result = Some(SpinorError::IdMismatch);
                        }
                    },
                    _ => {
                        wr.result = Some(SpinorError::NoId);
                    }
                }
                buffer.replace(wr).expect("couldn't return response code to WriteRegion");
            },
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    let quitconn = xous::connect(susres_mgr_sid).unwrap();
    xous::send_message(quitconn, xous::Message::new_scalar(api::SusResOps::Quit.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(quitconn).unwrap();}

    xns.unregister_server(spinor_sid).unwrap();
    xous::destroy_server(spinor_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
