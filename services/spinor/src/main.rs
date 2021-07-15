#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::*;
use xous_ipc::Buffer;
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack};

use core::sync::atomic::{AtomicBool, Ordering};

/*
OK, we have two approaches to doing this block.

1. we own all the read pages, and third party blocks send
requests to us to patch areas

2. we own nothing, and we can only patch areas that are pre-aligned
to word boundaries

We should do (2). Owning everything sucks for everything else.
However, the lib.rs side would be able to take a &[u8] that
represents the readable area, and then another &[u8] that represents
the data we want patched. And then it can format the requests to the
spinor block such that everything is aligned.

*/

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use susres::{RegManager, RegOrField, SuspendResume};
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use num_traits::*;

    enum FlashOp {
        EraseSector(u32), // 4k sector
        WritePage(u32, [u8; 256], u8), // page, data, len
        ReadId,
    }

    static SPINOR_RUNNING: AtomicBool = AtomicBool::new(false);
    static SPINOR_RESULT: AtomicU32 = AtomicU32::new(0);
    fn spinor_safe_context(_irq_no: usize, arg: *mut usize) {
        let spinor = unsafe { &mut *(arg as *mut Spinor) };

        let mut result = 0;
        match spinor.cur_op {
            Some(FlashOp::EraseSector(sector)) => {

            },
            Some(FlashOp::WritePage(page, data, len)) => {

            },
            Some(FlashOp::ReadId) => {
                let upper = flash_rdid(&mut spinor.csr, 2);
                let lower = flash_rdid(&mut spinor.csr, 1);
                // re-assemble the ID word from the duplicated bytes read
                result = (lower & 0xFF) | ((lower >> 8) & 0xFF00) | (upper & 0xFF_0000);
            },
            None => {
                panic!("Improper entry to SPINOR safe context.");
            }
        }

        spinor.cur_op = None;
        SPINOR_RESULT.store(result, Ordering::Relaxed);
        spinor.softirq.wfo(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, 1);
        SPINOR_RUNNING.store(false, Ordering::Relaxed);
    }

    fn ecc_handler(_irq_no: usize, arg: *mut usize) {
        let spinor = unsafe { &mut *(arg as *mut Spinor) };

        xous::try_send_message(spinor.handler_conn,
            xous::Message::new_scalar(Opcode::EccError.to_usize().unwrap(),
                spinor.csr.rf(utra::spinor::ECC_ADDRESS_ECC_ADDRESS) as usize,
                spinor.csr.rf(utra::spinor::ECC_STATUS_ECC_OVERFLOW) as usize,
                0, 0)).map(|_|()).unwrap();

        spinor.csr.wfo(utra::spinor::EV_PENDING_ECC_ERROR, 1);
    }

    fn flash_rdsr(csr: &mut utralib::CSR<u32>, lock_reads: u32) -> u32 {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
            | csr.ms(utra::spinor::COMMAND_LOCK_READS, lock_reads)
            | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x05) // RDSR
            | csr.ms(utra::spinor::COMMAND_DUMMY_CYCLES, 4)
            | csr.ms(utra::spinor::COMMAND_DATA_WORDS, 1)
            | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        csr.r(utra::spinor::CMD_RBK_DATA)
    }

    fn flash_rdscur(csr: &mut utralib::CSR<u32>) -> u32 {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
        csr.wo(utra::spinor::COMMAND,
              csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
            | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
            | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x2B) // RDSCUR
            | csr.ms(utra::spinor::COMMAND_DUMMY_CYCLES, 4)
            | csr.ms(utra::spinor::COMMAND_DATA_WORDS, 1)
            | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        csr.r(utra::spinor::CMD_RBK_DATA)
    }

    fn flash_rdid(csr: &mut utralib::CSR<u32>, offset: u32) -> u32 {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x9f)  // RDID
          | csr.ms(utra::spinor::COMMAND_DUMMY_CYCLES, 4)
          | csr.ms(utra::spinor::COMMAND_DATA_WORDS, offset) // 2 -> 0x3b3b8080, // 1 -> 0x8080c2c2
          | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
        csr.r(utra::spinor::CMD_RBK_DATA)
    }

    fn flash_wren(csr: &mut utralib::CSR<u32>) {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x06)  // WREN
          | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
    }

    fn flash_wrdi(csr: &mut utralib::CSR<u32>) {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, 0);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x04)  // WRDI
          | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
    }

    fn flash_se4b(csr: &mut utralib::CSR<u32>, sector_address: u32) {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, sector_address);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x21)  // SE4B
          | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
          | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
    }

    fn flash_be4b(csr: &mut utralib::CSR<u32>, block_address: u32) {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, block_address);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0xdc)  // BE4B
          | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
          | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
    }

    fn flash_pp4b(csr: &mut utralib::CSR<u32>, address: u32, data_bytes: u32) {
        csr.wfo(utra::spinor::CMD_ARG_CMD_ARG, address);
        csr.wo(utra::spinor::COMMAND,
            csr.ms(utra::spinor::COMMAND_EXEC_CMD, 1)
          | csr.ms(utra::spinor::COMMAND_CMD_CODE, 0x12)  // PP4B
          | csr.ms(utra::spinor::COMMAND_HAS_ARG, 1)
          | csr.ms(utra::spinor::COMMAND_DATA_WORDS, data_bytes / 2)
          | csr.ms(utra::spinor::COMMAND_LOCK_READS, 1)
        );
        while csr.rf(utra::spinor::STATUS_WIP) != 0 {}
    }

    pub struct Spinor {
        id: u32,
        handler_conn: xous::CID,
        csr: utralib::CSR<u32>,
        susres: RegManager::<{utra::spinor::SPINOR_NUMREGS}>,
        softirq: utralib::CSR<u32>,
        cur_op: Option<FlashOp>,
        ticktimer: ticktimer_server::Ticktimer,
        // TODO: refactor ecup command to use spinor to operate the reads
    }

    impl Spinor {
        pub fn new(handler_conn: xous::CID) -> Spinor {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::spinor::HW_SPINOR_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map SPINOR CSR range");
            let softirq = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::spinor_soft_int::HW_SPINOR_SOFT_INT_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map SPINOR soft interrupt CSR range");

            let mut spinor = Spinor {
                id: 0,
                handler_conn,
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                softirq: CSR::new(softirq.as_mut_ptr() as *mut u32),
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                cur_op: None,
                ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
            };

            xous::claim_interrupt(
                utra::spinor_soft_int::SPINOR_SOFT_INT_IRQ,
                spinor_safe_context,
                (&mut spinor) as *mut Spinor as *mut usize,
            )
            .expect("couldn't claim SPINOR irq");
            spinor.softirq.wfo(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, 1);
            spinor.softirq.wfo(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, 1);

            xous::claim_interrupt(
                utra::spinor::SPINOR_IRQ,
                ecc_handler,
                (&mut spinor) as *mut Spinor as *mut usize,
            )
            .expect("couldn't claim SPINOR irq");
            spinor.softirq.wfo(utra::spinor::EV_PENDING_ECC_ERROR, 1);
            spinor.softirq.wfo(utra::spinor::EV_ENABLE_ECC_ERROR, 1);
            spinor.susres.push_fixed_value(RegOrField::Reg(utra::spinor::EV_PENDING), 0xFFFF_FFFF);
            spinor.susres.push(RegOrField::Reg(utra::spinor::EV_ENABLE), None);

            // now populate the id field
            spinor.cur_op = Some(FlashOp::ReadId);
            SPINOR_RUNNING.store(true, Ordering::Relaxed);
            spinor.softirq.wfo(utra::spinor_soft_int::SOFTINT_SOFTINT, 1);
            while SPINOR_RUNNING.load(Ordering::Relaxed) {}
            spinor.id = SPINOR_RESULT.load(Ordering::Relaxed);

            spinor
        }

        /// changes into the spinor interrupt handler context, which is "safe" for ROM operations because we guarantee
        /// we don't touch the SPINOR block inside that interrupt context
        fn change_context(&mut self) -> u32 {
            if self.cur_op.is_none() {
                log::error!("change_context called with no spinor op set. This is an internal error...panicing!");
                panic!("change_context called with no spinor op set.");
            }
            self.ticktimer.ping_wdt();
            SPINOR_RUNNING.store(true, Ordering::Relaxed);
            self.softirq.wfo(utra::spinor_soft_int::SOFTINT_SOFTINT, 1);
            while SPINOR_RUNNING.load(Ordering::Relaxed) {
                // there is no timeout condition that makes sense. If we're in a very long flash op -- and they can take hundreds of ms --
                // simply timing out and trying to move on could lead to hardware damage as we'd be accessing a ROM that is in progress.
                // in other words: if the flash memory is broke, you're broke too, ain't nobody got time for that.
            }
            self.ticktimer.ping_wdt();
            SPINOR_RESULT.load(Ordering::Relaxed)
        }

        pub fn write_region(&mut self, wr: &mut WriteRegion) {
            if wr.start + wr.len > SPINOR_SIZE_BYTES {
                wr.result = Some(SpinorError::InvalidRequest);
                return;
            }

        }
        pub fn erase_region(&mut self, start_adr: u32, num_u8: u32) -> SpinorError {
            let mut erased = 0;

            let blocksize;
            if num_u8 - erased > 4096 {
                blocksize = 4096;
            } else {
                blocksize = 65536;
            }

            loop {
                flash_wren()?;
                let status = flash_rdsr(1)?;
                // println!("WREN: FLASH status register: 0x{:08x}", status);
                if status & 0x02 != 0 {
                    break;
                }
            }

            if blocksize <= 4096 {
                flash_se4b(addr + erased as u32)?;
            } else {
                flash_be4b(addr + erased as u32)?;
            }
            erased += blocksize;

            loop {
                let status = flash_rdsr(1)?;
                // println!("BE4B: FLASH status register: 0x{:08x}", status);
                if status & 0x01 == 0 {
                    break;
                }
            }

            let result = flash_rdscur()?;
            // println!("erase result: 0x{:08x}", result);
            if result & 0x60 != 0 {
                error!("E_FAIL/P_FAIL set, programming may have failed.")
            }

            if flash_rdsr(1)? & 0x02 != 0 {
                flash_wrdi()?;
                loop {
                    let status = flash_rdsr(1)?;
                    // println!("WRDI: FLASH status register: 0x{:08x}", status);
                    if status & 0x02 == 0 {
                        break;
                    }
                }
            }

            SpinorError::NoError
        }

        pub fn suspend(&mut self) {
            self.susres.suspend();
        }
        pub fn resume(&mut self) {
            self.susres.resume();
            self.softirq.wfo(utra::spinor_soft_int::EV_PENDING_SPINOR_INT, 1);
            self.softirq.wfo(utra::spinor_soft_int::EV_ENABLE_SPINOR_INT, 1);
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

    let handler_conn = xous::connect(spinor_sid).expect("couldn't create interrupt handler callback connection");
    let mut spinor = Spinor::new(handler_conn);

    log::trace!("ready to accept requests");

    // handle suspend/resume with a separate thread, which monitors our in-progress state
    // we can't interrupt an erase or program operation, so the op MUST finish before we can suspend.
    let susres_mgr_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = susres_mgr_sid.to_u32();
    xous::create_thread_4(susres_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize).expect("couldn't start susres handler thread");

    let mut client_id: Option<[u32; 4]> = None;
    let mut ecc_errors: [Option<u32>; 4] = [None, None, None, None]; // just record the first few errors, until we can get `std` and a convenient Vec/Queue

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
            Some(Opcode::EccError) => msg_scalar_unpack!(msg, address, _overflow, _, _, {
                // just some stand-in code -- should probably do something more clever, e.g. a rolling log
                // plus some error handling callback. But this is in the distant future once we have enough
                // of a system to eventually create such errors...
                log::error!("ECC error reported at 0x{:x}", address);
                let mut saved = false;
                for item in ecc_errors.iter_mut() {
                    if item.is_none() {
                        *item = Some(address as u32);
                        saved = true;
                        break;
                    }
                }
                if !saved {
                    log::error!("ran out of slots to record ECC errors");
                }
            }),
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
