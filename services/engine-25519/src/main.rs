#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::*;
use core::sync::atomic::{AtomicBool, Ordering};
use xous::msg_blocking_scalar_unpack;

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use susres::{ManagedMem, RegManager, RegOrField, SuspendResume};
    use num_traits::*;

    pub struct Engine25519 {
        csr: utralib::CSR<u32>,
        mem: xous::MemoryRange,
        susres: RegManager::<{utra::engine::ENGINE_NUMREGS}>,
        handler_conn: Option<xous::CID>,
        sr_backing: ManagedMem::<{utralib::generated::HW_ENGINE_MEM_LEN}>,
        mpc_resume: Option<u32>,
        clean_resume: Option<bool>,
    }
    fn handle_engine_irq(_irq_no: usize, arg: *mut usize) {
        let engine = unsafe { &mut *(arg as *mut Engine25519) };

        let reason = engine.csr.r(utra::engine::EV_PENDING);

        if let Some(conn) = engine.handler_conn {
            if reason & engine.csr.ms(utra::engine::EV_PENDING_FINISHED, 1) != 0 {
                xous::try_send_message(conn,
                    xous::Message::new_scalar(Opcode::EngineDone.to_usize().unwrap(),
                        0, 0, 0, 0)).map(|_|()).unwrap();
            }
            if reason & engine.csr.ms(utra::engine::EV_PENDING_ILLEGAL_OPCODE, 1) != 0 {
                xous::try_send_message(conn,
                    xous::Message::new_scalar(Opcode::IllegalOpcode.to_usize().unwrap(),
                        0, 0, 0, 0)).map(|_|()).unwrap();
            }
        } else {
            panic!("engine interrupt happened with a handler");
        }
        // clear the interrupt
        engine.csr
            .wo(utra::engine::EV_PENDING, reason);
    }
    impl Engine25519 {
        pub fn new(handler_conn: xous::CID) -> Engine25519 {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::engine::HW_ENGINE_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map engine CSR range");
            let mem = xous::syscall::map_memory(
                xous::MemoryAddress::new(utralib::HW_ENGINE_MEM),
                None,
                utralib::HW_ENGINE_MEM_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map engine memory window range");

            let mut engine = Engine25519 {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                mem,
                handler_conn: Some(handler_conn),
                sr_backing: ManagedMem::new(mem),
                mpc_resume: None,
                clean_resume: None,
            };

            xous::claim_interrupt(
                utra::engine::ENGINE_IRQ,
                handle_engine_irq,
                (&mut engine) as *mut Engine25519 as *mut usize,
            )
            .expect("couldn't claim Power irq");

            engine.csr.wo(utra::engine::EV_PENDING, 0xFFFF_FFFF); // clear any droppings.
            engine.csr.wo(utra::engine::EV_ENABLE,
                engine.csr.ms(utra::engine::EV_ENABLE_FINISHED, 1) |
                engine.csr.ms(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, 1)
            );

            // setup the susres context. Most of the defaults are fine, so they aren't explicitly initialized in the code above,
            // but they still have to show up down here in case we're suspended mid-op
            engine.susres.push(RegOrField::Reg(utra::engine::POWER), None); // on resume, this needs to be setup first, so that the "pause" state is captured correctly
            engine.susres.push(RegOrField::Reg(utra::engine::WINDOW), None);
            engine.susres.push(RegOrField::Reg(utra::engine::MPSTART), None);
            engine.susres.push(RegOrField::Reg(utra::engine::MPLEN), None);
            engine.susres.push_fixed_value(RegOrField::Reg(utra::engine::EV_PENDING), 0xFFFF_FFFF);
            engine.susres.push(RegOrField::Reg(utra::engine::EV_ENABLE), None);

            // engine.susres.push(RegOrField::Reg(utra::engine::CONTROL), None); // don't push this, we need to manually coordinate `mpcresume` before resuming

            engine
        }

        pub fn suspend(&mut self) {
            self.clean_resume = Some(false); // if this isn't set to try by the resume, then we've had a failure

            if self.csr.rf(utra::engine::STATUS_RUNNING) == 1 {
                // request a pause from the engine. it will stop executing at the next microcode op
                // and assert STATUS_PAUSE_GNT
                self.csr.rmwf(utra::engine::POWER_PAUSE_REQ, 1);
                while (self.csr.rf(utra::engine::STATUS_PAUSE_GNT) == 0) && (self.csr.rf(utra::engine::STATUS_RUNNING) == 1) {
                    // busy wait for this to clear, or the engine to stop running; should happen in << 1us
                    if self.csr.rf(utra::engine::STATUS_PAUSE_GNT) == 1 {
                        // store the current PC value as a resume note
                        self.mpc_resume = Some(self.csr.rf(utra::engine::STATUS_MPC));
                    } else {
                        // the implication here is the engine actually finished its last opcode, so it would not enter the paused state;
                        // rather, it is now stopped.
                        self.mpc_resume = None;
                    }
                }

            } else {
                self.mpc_resume = None;
            }
            // copy the ucode & rf out to the backing memory
            self.sr_backing.suspend();
            // now backup all the machine registers
            self.susres.suspend();
        }
        pub fn resume(&mut self) {
            // "power" should be resumed first by the restore, which would set the pause bit, if it was previously set
            self.susres.resume();
            self.sr_backing.resume();

            // in the case of a resume from pause, we need to specify the PC to resume from
            // clear the pause
            if let Some(mpc) = self.mpc_resume {
                if self.csr.rf(utra::engine::POWER_PAUSE_REQ) != 1 {
                    log::error!("resuming from an unexpected state: we had mpc of {} set, but pause was not requested!", mpc);
                    self.clean_resume = Some(false);
                    // we don't resume execution. Presumably this will cause terrible things to happen such as
                    // the interrupt waiting for execution to be done to never trigger.
                    // perhaps we could try to trigger that somehow...?
                } else {
                    // the pause was requested, but crucially, the engine was not in the "go" state. This means that
                    // the engine will get its starting PC from the resume PC when we hit go again, instead of the mpstart register.
                    self.csr.wfo(utra::engine::MPRESUME_MPRESUME, mpc);
                    // start the engine
                    self.csr.wfo(utra::engine::CONTROL_GO, 1);
                    // it should grab the PC from `mpresume` and then go to the paused state. Wait until
                    // we have achieved the identical paused state that happened before resume, before unpausing!
                    while self.csr.rf(utra::engine::STATUS_PAUSE_GNT) == 0 {
                        // this should be very fast, within a couple CPU cycles
                    }
                    self.clean_resume = Some(true); // note that we had a clean resume before resuming the execution
                    // this resumes execution of the CPU
                    self.csr.rmwf(utra::engine::POWER_PAUSE_REQ, 0);
                }
            } else {
                // if we didn't have a resume PC set, we weren't paused, so we just continue on our merry way.
                self.clean_resume = Some(true);
            }
        }
        const UCODE_U8_BASE: usize = 0x0;
        const UCODE_U32_BASE: usize = 0x0;
        const UCODE_U32_SIZE: usize = (0x1_0000 / 4);
        const RF_U8_BASE: usize = 0x1_0000;
        const RF_U32_BASE: usize = (0x1_0000 / 4);
        const RF_U32_SIZE: usize = (0x4000 / 4);

        pub fn run(&mut self, job: Job) {
            // create a pointer to the entire memory window range
            let mem_window: &mut [u32; (utralib::HW_ENGINE_MEM_LEN / 4)] = self.mem.as_mut_ptr() as *mut [u32; (utralib::HW_ENGINE_MEM_LEN / 4)];
            // parcel out mutable subslices
            // first, split into the ucode/rf areas
            let (ucode_superblock, rf_superblock) = (&mem_window).split_at_mut(RF_U32_BASE);
            // further reduce the ucode area to just the unaliased ucode region
            let (ucode, _) = ucode_superblock.split_at_mut(UCODE_U32_SIZE);
            // further reduce the rf area to just the unaliased rf region
            let (rf, _) = rf_superblock.split_at_mut(RF_U32_SIZE);

            let window = if let Some(w) = job.window {
                w as usize
            } else {
                0 as usize // default window is 0
            };

            // this should "just panic" if we have a bad window arg, which is the desired behavior
            for (&src, dst) in job.rf.iter().zip(rf[window * RF_SIZE_IN_U32..(window+1) * RF_SIZE_IN_U32].iter_mut()) {
                *dst = src;
            }
            // copy in the microcode
            for (&src, dst) in job.ucode.iter().zip(ucode.iter_mut()) {
                *dst = src;
            }
            self.csr.wfo(utra::engine::WINDOW_WINDOW, window as u32); // this value should now be validated because an invalid window would cause a panic on slice copy
            self.csr.wfo(utra::engine::MPSTART_MPSTART, job.uc_start);
            self.csr.wfo(utra::engine::MPLEN_MPLEN, job.uc_len);

            // this will start the run. interrupts should *already* be enabled for the completion notification...
            self.csr.wfo(utra::engine::CONTROL_GO, 1);
        }

        pub fn get_result(&mut self) -> JobResult {
            if let Some(clean_resume) = self.clean_resume {
                if !clean_resume {
                    return JobResult::SuspendError;
                }
            }

            // create a pointer to the entire memory window range
            let mem_window: &mut [u32; (utralib::HW_ENGINE_MEM_LEN / 4)] = self.mem.as_mut_ptr() as *mut [u32; (utralib::HW_ENGINE_MEM_LEN / 4)];
            // parcel out mutable subslices
            // first, split into the ucode/rf areas
            let (_ucode_superblock, rf_superblock) = (&mem_window).split_at_mut(RF_U32_BASE);
            // further reduce the ucode area to just the unaliased ucode region
            let (rf, _) = rf_superblock.split_at_mut(RF_U32_SIZE);

            let mut ret_rf: [u32; RF_SIZE_IN_U32] = [0; RF_SIZE_IN_U32];
            let window = self.csr.rf(utra::engine::WINDOW_WINDOW) as usize;
            for (&src, dst) in rf[window * RF_SIZE_IN_U32..(window+1) * RF_SIZE_IN_U32].iter().zip(ret_rf.iter_mut()) {
                *dst = src;
            }

            JobResult::Result(ret_rf)
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Engine25519 {
    }

    impl Engine25519 {
        pub fn new() -> Engine25519 {
            Engine25519 {
            }
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
        pub fn run(&mut self, _job: Job) {
        }
        pub fn get_result(&mut self) -> JobResult {
            JobResult::IllegalOpcodeException
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Engine25519;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let engine25519_sid = xns.register_name(api::SERVER_NAME_ENGINE25519, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", engine25519_sid);

    let handler_conn = xous::connect(engine25519_sid).expect("couldn't create IRQ handler connection");
    let mut engine25519 = Engine25519::new(handler_conn);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(engine25519_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    let mut client_cid: Option<xous::CID> = None;
    loop {
        let msg = xous::receive_message(engine25519_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                engine25519.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                engine25519.resume();
            }),
            Some(Opcode::RunJob) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let job = buffer.to_original::<Job, _>().unwrap();

                let response = if client_id.is_none() {
                    client_cid = Some(xous::connect(job.id).expect("couldn't connect to the caller's server"));
                    engine25519.run(job);
                    JobResult::Started
                } else {
                    JobResult::EngineUnavailable
                };
                buffer.replace(response).unwrap();
            },
            Some(Opcode::IsFree) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if client_id.is_none() {
                    xous::return_scalar(msg.sender, 1).expect("couldn't return IsIdle query");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't return IsIdle query");
                }
            }),
            Some(Opcode::EngineDone) => {
                if let Some(cid) = client_cid {
                    let result = engine25519.get_result();
                    let buf = Buffer::into_buf(result).or(Err(xous::Error::InternalError))?;
                    buf.send(cid, Return::Result.to_u32().unwrap()).expect("couldn't return result to caller");

                    // this simultaneously releases the lock and disconnects from the caller
                    unsafe{xous::disconnect(client_cid.take()).expect("couldn't disconnect from the caller");}
                } else {
                    log::error!("illegal state: got a result, but no client was registered??");
                }
            },
            Some(Opcode::IllegalOpcode) => {
                let buf = Buffer::into_buf(JobResult::IllegalOpcodeException).or(Err(xous::Error::InternalError))?;
                buf.send(cid, Return::Result.to_u32().unwrap()).expect("couldn't return result to caller");
            }
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
    xns.unregister_server(engine25519_sid).unwrap();
    xous::destroy_server(engine25519_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
