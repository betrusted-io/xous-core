#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::*;
use core::sync::atomic::{AtomicBool, Ordering};
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;

static RUN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static DISALLOW_SUSPEND: AtomicBool = AtomicBool::new(false);
static SUSPEND_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use susres::{RegManager, RegOrField, SuspendResume};
    use num_traits::*;
    //use core::num::NonZeroUsize;
    use core::sync::atomic::Ordering;
    use crate::RUN_IN_PROGRESS;
    use crate::DISALLOW_SUSPEND;

    pub struct Engine25519Hw {
        csr: utralib::CSR<u32>,
        // these are slices mapped directly to the hardware memory space
        ucode_hw: &'static mut [u32],
        rf_hw: &'static mut [u32],
        susres: RegManager::<{utra::engine::ENGINE_NUMREGS}>,
        handler_conn: Option<xous::CID>,
        // don't use the susres ManagedMem primitive; it blows out the stack. Instead, heap-allocate the backing stores.
        ucode_backing: &'static mut [u32],
        rf_backing: &'static mut [u32],
        mpc_resume: Option<u32>,
        clean_resume: Option<bool>,
        do_notify: bool,
        illegal_opcode: bool,
    }
    fn handle_engine_irq(_irq_no: usize, arg: *mut usize) {
        let engine = unsafe { &mut *(arg as *mut Engine25519Hw) };

        let reason = engine.csr.r(utra::engine::EV_PENDING);
        RUN_IN_PROGRESS.store(false, Ordering::Relaxed);
        if reason & engine.csr.ms(utra::engine::EV_PENDING_ILLEGAL_OPCODE, 1) != 0 {
            engine.illegal_opcode = true;
        } else {
            engine.illegal_opcode = false;
        }

        if engine.do_notify {
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
                panic!("engine interrupt happened without a handler");
            }
        }
        // clear the interrupt
        engine.csr
            .wo(utra::engine::EV_PENDING, reason);
    }

    impl Engine25519Hw {
        pub fn new(handler_conn: xous::CID) -> Engine25519Hw {
            assert!(TOTAL_RF_SIZE_IN_U32 == RF_TOTAL_U32_SIZE, "sanity check has failed on logical dimensions of register file vs hardware aperture sizes");

            log::trace!("creating engine25519 CSR");
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::engine::HW_ENGINE_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map engine CSR range");
            log::trace!("creating engine25519 memrange");
            let mem = xous::syscall::map_memory(
                xous::MemoryAddress::new(utralib::HW_ENGINE_MEM),
                None,
                utralib::HW_ENGINE_MEM_LEN,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map engine memory window range");

            let ucode_mem = xous::syscall::map_memory(
                None,
                None,
                UCODE_U8_SIZE,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ).expect("couldn't map backing store for microcode");
            let rf_mem = xous::syscall::map_memory(
                None,
                None,
                RF_TOTAL_U8_SIZE,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            ).expect("couldn't map RF backing store");
            let mut engine = Engine25519Hw {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                ucode_hw: unsafe{core::slice::from_raw_parts_mut(mem.as_mut_ptr().add(UCODE_U8_BASE) as *mut u32, UCODE_U32_SIZE)},
                rf_hw: unsafe{core::slice::from_raw_parts_mut(mem.as_mut_ptr().add(RF_U8_BASE) as *mut u32, RF_TOTAL_U32_SIZE)},
                handler_conn: Some(handler_conn),
                ucode_backing: unsafe{core::slice::from_raw_parts_mut(ucode_mem.as_mut_ptr() as *mut u32, UCODE_U32_SIZE)},
                rf_backing: unsafe{core::slice::from_raw_parts_mut(rf_mem.as_mut_ptr() as *mut u32, RF_TOTAL_U32_SIZE)},
                mpc_resume: None,
                clean_resume: None,
                do_notify: false,
                illegal_opcode: false,
            };

            log::trace!("claiming interrupt");
            xous::claim_interrupt(
                utra::engine::ENGINE_IRQ,
                handle_engine_irq,
                (&mut engine) as *mut Engine25519Hw as *mut usize,
            )
            .expect("couldn't claim engine irq");

            log::trace!("enabling interrupt");
            engine.csr.wo(utra::engine::EV_PENDING, 0xFFFF_FFFF); // clear any droppings.
            engine.csr.wo(utra::engine::EV_ENABLE,
                engine.csr.ms(utra::engine::EV_ENABLE_FINISHED, 1) |
                engine.csr.ms(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, 1)
            );

            // setup the susres context. Most of the defaults are fine, so they aren't explicitly initialized in the code above,
            // but they still have to show up down here in case we're suspended mid-op
            log::trace!("setting up susres");
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
            // accessing ucode & rf requires clocks to be on
            let orig_state = if self.csr.rf(utra::engine::POWER_ON) == 0 {
                self.csr.rmwf(utra::engine::POWER_ON, 1);
                false
            } else {
                true
            };

            // copy the ucode & rf into the backing memory
            for (&src, dst) in self.rf_hw.iter().zip(self.rf_backing.iter_mut()) {
                *dst = src;
            }
            for (&src, dst) in self.ucode_hw.iter().zip(self.ucode_backing.iter_mut()) {
                *dst = src;
            }

            // restore the power state setting
            if !orig_state {
                self.csr.rmwf(utra::engine::POWER_ON, 0);
            }
            // now backup all the machine registers
            self.susres.suspend();
        }
        pub fn resume(&mut self) {
            self.susres.resume();
            // if the power wasn't on, we have to flip it on temporarily to access the backing memories
            let orig_state = if self.csr.rf(utra::engine::POWER_ON) == 0 {
                self.csr.rmwf(utra::engine::POWER_ON, 1);
                false
            } else {
                true
            };

            // restore ucode & rf
            for (&src, dst) in self.rf_backing.iter().zip(self.rf_hw.iter_mut()) {
                *dst = src;
            }
            for (&src, dst) in self.ucode_backing.iter().zip(self.ucode_hw.iter_mut()) {
                *dst = src;
            }

            // restore the power state setting
            if !orig_state {
                self.csr.rmwf(utra::engine::POWER_ON, 0);
            }

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

        #[allow(dead_code)]
        pub(crate) fn power_dbg(&self) -> u32 {
            self.csr.r(utra::engine::POWER)
        }

        pub fn power_on(&mut self, on: bool) {
            if on {
                self.csr.rmwf(utra::engine::POWER_ON, 1);
            } else {
                self.csr.rmwf(utra::engine::POWER_ON, 0);
            }
        }
        pub fn run(&mut self, job: Job) {
            log::trace!("entering run");
            // block any suspends from happening while we set up the engine
            DISALLOW_SUSPEND.store(true, Ordering::Relaxed);

            log::trace!("extracting windows");
            let window = if let Some(w) = job.window {
                w as usize
            } else {
                0 as usize // default window is 0
            };
            log::trace!("using window {}", window);
            // this should "just panic" if we have a bad window arg, which is the desired behavior
            //let mut num = 0;
            for (&src, dst) in job.rf.iter().zip(self.rf_hw[window * RF_SIZE_IN_U32..(window+1) * RF_SIZE_IN_U32].iter_mut()) {
                //*dst = src;  // this gets optimized away, so replace it with the below line of code
                // since these are given to us by an iter, and we aren't doing .add() etc. on the pointers, should be safe.
                unsafe { (dst as *mut u32).write_volatile(src) };

                // performance critical, so comment it out if not using this debug code
                /*if src != 0 {
                    log::trace!("rf {}: 0x{:08x}", num, src);
                }
                num += 1;*/
            }
            // copy in the microcode
            //num = 0;
            for (&src, dst) in job.ucode.iter().zip(self.ucode_hw.iter_mut()) {
                //*dst = src;  // this gets optimized away, so replace it with the below line of code
                // since these are given to us by an iter, and we aren't doing .add() etc. on the pointers, should be safe.
                unsafe { (dst as *mut u32).write_volatile(src) };

                /*if src != 0 {
                    log::trace!("ucode {}: 0x{:08x}", num, src);
                }
                num += 1;*/
            }
            self.csr.wfo(utra::engine::WINDOW_WINDOW, window as u32); // this value should now be validated because an invalid window would cause a panic on slice copy
            self.csr.wfo(utra::engine::MPSTART_MPSTART, job.uc_start);
            self.csr.wfo(utra::engine::MPLEN_MPLEN, job.uc_len);

            log::trace!("sanity check uc{:08x}, rf{:08x}", self.ucode_hw[0], self.rf_hw[0]);

            // determine if this a sync or async call
            if job.id.is_some() {
                // async calls need a notification message
                self.do_notify = true;
            } else {
                // sync calls poll a state variable, and thus no message is sent
                self.do_notify = false;
            }
            // setup the sync polling variable
            RUN_IN_PROGRESS.store(true, Ordering::Relaxed);
            // this will start the run. interrupts should *already* be enabled for the completion notification...
            self.csr.wfo(utra::engine::CONTROL_GO, 1);

            // we are now in a stable config, suspends are allowed
            DISALLOW_SUSPEND.store(false, Ordering::Relaxed);
        }

        pub fn get_result(&mut self) -> JobResult {
            if let Some(clean_resume) = self.clean_resume {
                if !clean_resume {
                    return JobResult::SuspendError;
                }
            }
            if self.illegal_opcode {
                return JobResult::IllegalOpcodeException;
            }

            let mut ret_rf: [u32; RF_SIZE_IN_U32] = [0; RF_SIZE_IN_U32];
            let window = self.csr.rf(utra::engine::WINDOW_WINDOW) as usize;
            //let mut num = 0;
            for (&src, dst) in self.rf_hw[window * RF_SIZE_IN_U32..(window+1) * RF_SIZE_IN_U32].iter().zip(ret_rf.iter_mut()) {
                //*dst = src;  // this gets optimized away, so replace it with the below line of code
                // since these are given to us by an iter, and we aren't doing .add() etc. on the pointers, should be safe.
                unsafe { (dst as *mut u32).write_volatile(src) };
                /*
                if src != 0 {
                    log::debug!("result {}: 0x{:08x}", num, src);
                }
                num += 1;*/
            }

            JobResult::Result(ret_rf)
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Engine25519Hw {
    }

    impl Engine25519Hw {
        pub fn new() -> Engine25519Hw {
            Engine25519Hw {
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


fn susres_thread(engine_arg: usize) {
    use crate::implementation::Engine25519Hw;
    let engine25519 = unsafe { &mut *(engine_arg as *mut Engine25519Hw) };

    let susres_sid = xous::create_server().unwrap();
    let xns = xous_names::XousNames::new().unwrap();

    // register a suspend/resume listener
    let sr_cid = xous::connect(susres_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, api::SusResOps::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    log::trace!("starting engine25519 suspend/resume manager loop");
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SusResOps::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                // prevent new jobs from starting while we're in suspend
                // we do this first, because the race condition we're trying to catch is between a job
                // being set up, and it running.
                SUSPEND_IN_PROGRESS.store(true, Ordering::Relaxed);

                // this check will catch the case that a job happened to be started before we could set
                // our flag above.
                while DISALLOW_SUSPEND.load(Ordering::Relaxed) {
                    // don't start a suspend if we're in the middle of a critical region
                    xous::yield_slice();
                }

                // at this point:
                //  - there should be no new jobs in progress
                //  - any job that was being set up, will have been set up so its safe to interrupt execution
                engine25519.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                engine25519.resume();
                SUSPEND_IN_PROGRESS.store(false, Ordering::Relaxed);
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
    use crate::implementation::Engine25519Hw;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Debug);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let engine25519_sid = xns.register_name(api::SERVER_NAME_ENGINE25519, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", engine25519_sid);

    let handler_conn = xous::connect(engine25519_sid).expect("couldn't create IRQ handler connection");
    log::trace!("creating engine25519 object");
    let mut engine25519 = Engine25519Hw::new(handler_conn);

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    xous::create_thread_1(susres_thread, (&mut engine25519) as *mut Engine25519Hw as usize).expect("couldn't start susres handler thread");

    let mut client_cid: Option<xous::CID> = None;
    loop {
        let mut msg = xous::receive_message(engine25519_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::RunJob) => {
                // don't start a new job if a suspend is in progress
                while SUSPEND_IN_PROGRESS.load(Ordering::Relaxed) {
                    log::trace!("waiting for suspend to finish");
                    xous::yield_slice();
                }
                engine25519.power_on(true);

                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let job = buffer.to_original::<Job, _>().unwrap();

                let response = if client_cid.is_none() {
                    if let Some(job_id) = job.id {
                        log::trace!("running async job");
                        // async job
                        // the presence of an ID indicates we are doing an async method
                        client_cid = Some(xous::connect(xous::SID::from_array(job_id)).expect("couldn't connect to the caller's server"));
                        engine25519.run(job);
                        // just let the caller know we started a job, but don't return any results
                        JobResult::Started
                    } else {
                        // log::trace!("running sync job {:?}", job);
                        // sync job
                        // start the job, which should set RUN_IN_PROGRESS to true
                        engine25519.run(job);
                        while RUN_IN_PROGRESS.load(Ordering::Relaxed) {
                            // block until the job is done
                            xous::yield_slice();
                        }
                        let result = engine25519.get_result(); // return the result
                        engine25519.power_on(false);
                        result
                    }
                } else {
                    JobResult::EngineUnavailable
                };
                buffer.replace(response).unwrap();
            },
            Some(Opcode::IsFree) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if client_cid.is_none() {
                    xous::return_scalar(msg.sender, 1).expect("couldn't return IsIdle query");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't return IsIdle query");
                }
            }),
            Some(Opcode::EngineDone) => {
                if let Some(cid) = client_cid {
                    let result = engine25519.get_result();
                    let buf = Buffer::into_buf(result).or(Err(xous::Error::InternalError)).unwrap();
                    buf.send(cid, Return::Result.to_u32().unwrap()).expect("couldn't return result to caller");

                    // this simultaneously releases the lock and disconnects from the caller
                    unsafe{xous::disconnect(client_cid.take().unwrap()).expect("couldn't disconnect from the caller");}
                } else {
                    log::error!("illegal state: got a result, but no client was registered. Did we forget to disable interrupts on a synchronous call??");
                }
                engine25519.power_on(false);
            },
            Some(Opcode::IllegalOpcode) => {
                if let Some(cid) = client_cid {
                    let buf = Buffer::into_buf(JobResult::IllegalOpcodeException).or(Err(xous::Error::InternalError)).unwrap();
                    buf.send(cid, Return::Result.to_u32().unwrap()).expect("couldn't return result to caller");
                } else {
                    log::error!("illegal state: got a result, but no client was registered. Did we forget to disable interrupts on a synchronous call??");
                }
                engine25519.power_on(false);
            }
            Some(Opcode::Quit) => {
                log::info!("Received quit opcode, exiting!");
                engine25519.power_on(false);
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
