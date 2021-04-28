#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod os_timer;

mod api;
use api::*;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack, send_message, Message};
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};

#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use crate::SHOULD_RESUME;
    use core::sync::atomic::{AtomicBool, Ordering};

    // we do all the suspend/resume coordination in an interrupt context
    // the process state is pushed to main memory prior to entering this routine, so it's staged for a resume
    // on a clean suspend then resume, the loader will queue up this interrupt to be the first thing to
    // run on Xous resume, with a bit in the RESUME register set.
    fn susres_handler(_irq_no: usize, arg: *mut usize) {
        let sr = unsafe{ &mut *(arg as *mut SusResHw) };
        // clear the pending interrupt
        sr.csr.wfo(utra::susres::EV_PENDING_SOFT_INT, 1);

        if sr.csr.rf(utra::susres::STATE_RESUME) == 0 {
            // power the system down - this should result in an almost immediate loss of power
            sr.csr.wfo(utra::susres::POWERDOWN_POWERDOWN, 1);

            loop {} // block forever here
        } else {
            // this unblocks the threads waiting on the resume
            SHOULD_RESUME.store(true, Ordering::Relaxed);
        }
    } // leaving this context will initiate the resume code

    pub struct SusResHw {
        /// our CSR
        csr: utralib::CSR<u32>,
        /// memory region for the "clean suspend" marker
        marker: xous::MemoryRange,
        /// loader stack region -- this data is dirtied on every resume; claim it in this process so no others accidentally use it
        loader_stack: xous::MemoryRange,
        /// backing store for the ticktimer value
        stored_time: Option<u64>,
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
                stored_time: None,
                marker,
                loader_stack,
            };

            // check that the marker has been zero'd by map_memory. Once this passes we can probably get rid of this code.
            let check_marker =  marker.as_ptr() as *const [u32; 1024];
            for words in 0..1024 {
                if unsafe{(*check_marker)[words]} != 0 {
                    log::error!("marker had non-zero entry: 0x{:x} @ 0x{:x}", unsafe{(*check_marker)[words]}, words);
                }
            }

            xous::claim_interrupt(
                utra::susres::SUSRES_IRQ,
                susres_handler,
                (&mut sr) as *mut SusResHw as *mut usize,
            ).expect("couldn't claim IRQ");

            sr
        }

        pub fn do_suspend(&mut self, _forced: bool) {
            // make sure we're able to handle a soft interrupt
            self.csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, 1);

            // ensure that the resume bit is not set
            self.csr.wfo(utra::susres::STATE_RESUME, 0);

            // stop thread scheduling, by stopping the ticktimer
            self.csr.wfo(utra::susres::CONTROL_PAUSE, 1);
            self.stored_time = Some(
               (self.csr.r(utra::susres::TIME0) as u64 |
               (self.csr.r(utra::susres::TIME1) as u64) << 32)
               + 1 // advance by one tick, to ensure time goes up monotonically at the next resume
            );

            // setup the clean suspend marker, note if things were forced

            // set a wakeup alarm

            // trigger an interrupt to process the final suspend bits
            self.csr.wfo(utra::susres::INTERRUPT_INTERRUPT, 1);

            // SHOULD_RESUME will be set true by the interrupt context when it re-enters from resume
            while !SHOULD_RESUME.load(Ordering::Relaxed) {
                xous::yield_slice();
            }
        }
        pub fn do_resume(&mut self, _forced: bool) {
            if let Some(time)= self.stored_time.take() {
                // restore the ticktimer
                self.csr.wo(utra::susres::RESUME_TIME0, time as u32);
                self.csr.wo(utra::susres::RESUME_TIME1, (time >> 32) as u32);
                // load the saved value -- can only be done while the timer is paused
                self.csr.wo(utra::susres::CONTROL,
                    self.csr.ms(utra::susres::CONTROL_PAUSE, 1) |
                    self.csr.ms(utra::susres::CONTROL_LOAD, 1)
                );
                // start the timer running
                self.csr.wo(utra::susres::CONTROL, 0);
            } else {
                panic!("Can't resume because the ticktimer value was not saved properly before suspend!")
            }
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
        pub fn do_suspend(&mut self, _forced: bool) {
        }
        pub fn do_resume(&mut self, _forced: bool) {
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
    ready_to_suspend: bool,
    token: u32,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum TimeoutOpcode {
    SetCsr,
    Run,
    Drop,
}

static TIMEOUT_TIME: AtomicU32 = AtomicU32::new(250);
static TIMEOUT_CONN: AtomicU32 = AtomicU32::new(0);
pub fn timeout_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    #[cfg(target_os = "none")]
    use utralib::generated::*;
    #[cfg(target_os = "none")]
    let mut csr: Option<CSR::<u32>> = None;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            #[cfg(target_os = "none")]
            Some(TimeoutOpcode::SetCsr) => msg_scalar_unpack!(msg, base, _, _, _, {
                csr = Some(CSR::new(base as *mut u32));
            }),
            #[cfg(not(target_os = "none"))]
            Some(TimeoutOpcode::SetCsr) => msg_scalar_unpack!(msg, base, _, _, _, {
                // ignore the opcode in hosted mode
            }),
            Some(TimeoutOpcode::Run) => {
                #[cfg(target_os = "none")]
                {
                    // we have to re-implement the ticktimer time reading here because as we wait for the timeout,
                    // the ticktimer goes away! so we use the susres local copy with direct hardware ops to keep track of time in this phase
                    fn get_hw_time(hw: CSR::<u32>) -> u64 {
                        hw.r(utra::susres::TIME0) as u64 | ((hw.r(utra::susres::TIME1) as u64) << 32)
                    }
                    if let Some(hw) = csr {
                        let now = get_hw_time(hw);
                        let timeout = TIMEOUT_TIME.load(Ordering::Relaxed); // ignore updates to timeout once we're waiting
                        while ((get_hw_time(hw) - now) as u32) < timeout {
                            xous::yield_slice();
                        }
                    } else {
                        panic!("hardware CSR not sent to timeout_thread before it was instructed to run");
                    }
                }
                match send_message(TIMEOUT_CONN.load(Ordering::Relaxed),
                    Message::new_scalar(Opcode::SuspendTimeout.to_usize().unwrap(), 0, 0, 0, 0)
                ) {
                    Err(xous::Error::ServerNotFound) => break,
                    Ok(xous::Result::Ok) => {},
                    _ => panic!("unhandled error in status pump thread")
                }
            }
            Some(TimeoutOpcode::Drop) => {
                break
            }
            None => {
                log::error!("received unknown opcode in timeout_thread!");
            }
        }
    }
    unsafe{xous::disconnect(TIMEOUT_CONN.load(Ordering::Relaxed))};
    TIMEOUT_CONN.store(0, Ordering::Relaxed);
    xous::destroy_server(sid);
}

static SHOULD_RESUME: AtomicBool = AtomicBool::new(false);
static RESUME_EXEC: AtomicBool = AtomicBool::new(false);
pub fn execution_gate() {
    let xns = xous_names::XousNames::new().unwrap();
    let execgate_sid = xns.register_name(api::SERVER_NAME_EXEC_GATE).expect("can't register execution gate");
    log::trace!("execution_gate registered with NS -- {:?}", execgate_sid);

    loop {
        let msg = xous::receive_message(execgate_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            // the entire purpose of SupendingNow is to block the thread that sent the message, until we're ready to resume.
            Some(ExecGateOpcode::SuspendingNow) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                while !RESUME_EXEC.load(Ordering::Relaxed) {
                    xous::yield_slice();
                }
            }),
            Some(ExecGateOpcode::Drop) => {
                break;
            }
            None => {
                log::error!("received unknown opcode in execution_gate");
            }
        }
    }
    xous::destroy_server(execgate_sid);
}

#[xous::xous_main]
fn xmain() -> ! {
    // Start the OS timer which is responsible for setting up preemption.
    os_timer::init();

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let susres_sid = xns.register_name(api::SERVER_NAME_SUSRES).expect("can't register server");
    log::trace!("main loop registered with NS -- {:?}", susres_sid);

    // make a connection for the timeout thread to wake us up
    let timeout_incoming_conn = xous::connect(susres_sid).unwrap();
    TIMEOUT_CONN.store(timeout_incoming_conn, Ordering::Relaxed);
    // allocate a private server ID for the timeout thread, it's not registered with the name server
    let timeout_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = timeout_sid.to_u32();
    xous::create_thread_4(timeout_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize).expect("couldn't create timeout thread");
    let timeout_outgoing_conn = xous::connect(timeout_sid).expect("couldn't connect to our timeout thread");

    let mut susres_hw = implementation::SusResHw::new();
    let mut suspend_requested = false;
    let mut timeout_pending = false;

    let mut suspend_subscribers: [Option<ScalarCallback>; 32] = [None; 32];
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendEventSubscribe) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut suspend_subscribers);
            },
            Some(Opcode::SuspendReady) => msg_scalar_unpack!(msg, token, _, _, _, {
                if !suspend_requested {
                    log::error!("received a SuspendReady message when a suspend wasn't pending. Ignoring.");
                    continue;
                }
                if token >= suspend_subscribers.len() {
                    panic!("received a SuspendReady token that's out of range");
                }
                if let Some(mut scb) = suspend_subscribers[token] {
                    if scb.ready_to_suspend {
                        log::error!("received a duplicate SuspendReady token: {} from {:?}", token, scb);
                    }
                    scb.ready_to_suspend = true;

                    let mut all_ready = true;
                    for maybe_sub in suspend_subscribers.iter() {
                        if let Some(sub) = maybe_sub {
                            if sub.ready_to_suspend == false {
                                all_ready = false;
                                break;
                            }
                        };
                    }
                    if all_ready {
                        susres_hw.do_suspend(false);
                        // when do_suspend() returns, it means we've resumed
                        suspend_requested = false;
                        susres_hw.do_resume(false);
                        // this now allows all other threads to commence
                        RESUME_EXEC.store(true, Ordering::Relaxed);
                    }
                } else {
                    panic!("received an invalid token that does not map to a registered suspend listener");
                }
            }),
            Some(Opcode::SuspendRequest) => {
                suspend_requested = true;
                // clear the resume gate
                SHOULD_RESUME.store(false, Ordering::Relaxed);
                RESUME_EXEC.store(false, Ordering::Relaxed);
                // clear the ready to suspend flag
                for maybe_sub in suspend_subscribers.iter_mut() {
                    if let Some(sub) = maybe_sub {
                        sub.ready_to_suspend = false;
                    };
                }
                // do we want to start the timeout before or after sending the notifications? hmm. 🤔
                timeout_pending = true;
                send_message(timeout_outgoing_conn,
                    Message::new_scalar(TimeoutOpcode::Run.to_usize().unwrap(), 0, 0, 0, 0)
                ).expect("couldn't initiate timeout before suspend!");

                send_event(&suspend_subscribers);
            },
            Some(Opcode::SuspendTimeout) => {
                if timeout_pending {
                    timeout_pending = false;
                    // force a suspend
                    susres_hw.do_suspend(true);
                    // when do_suspend() returns, it means we've resumed
                    suspend_requested = false;
                    susres_hw.do_resume(true);
                    RESUME_EXEC.store(true, Ordering::Relaxed);
                } else {
                    // this means we did a clean suspend, we've resumed, and the timeout came back after the resume
                    // just ignore the message.
                }
            }
            Some(Opcode::Quit) => {
                break
            }
            None => {
                log::error!("couldn't convert opcode");
            }
        }
    }
    // clean up our program
    unhook(&mut suspend_subscribers);
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(susres_sid).unwrap();
    xous::destroy_server(susres_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let mut cb_dat = ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
        ready_to_suspend: false,
        token: 0,
    };
    for i in 0..cb_conns.len() {
        if cb_conns[i].is_none() {
            cb_dat.token = i as u32;
            cb_conns[i] = Some(cb_dat);
            return;
        }
    }
    log::error!("ran out of space registering callback");
}
fn unhook(cb_conns: &mut [Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter_mut() {
        if let Some(scb) = entry {
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_blocking_scalar(SuspendEventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
            ).unwrap();
            unsafe{xous::disconnect(scb.server_to_cb_cid).unwrap();}
        }
        *entry = None;
    }
}
fn send_event(cb_conns: &[Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter() {
        if let Some(scb) = entry {
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(SuspendEventCallback::Event.to_usize().unwrap(),
                   scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, scb.token as usize, 0)
            ).unwrap();
        };
    }
}
