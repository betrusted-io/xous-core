#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod murmur3;

mod api;
use api::{Opcode, ScalarHook, SuspendEventCallback, ExecGateOpcode};

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack, send_message, Message};
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};

#[cfg(feature = "debugprint")]
#[macro_use]
mod debug;

// effectively ignore any println! macros when debugprint is not selected
#[cfg(not(feature = "debugprint"))]
macro_rules! println
{
	() => ({
	});
	($fmt:expr) => ({
	});
	($fmt:expr, $($args:tt)+) => ({
	});
}


#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    use crate::murmur3::murmur3_32;
    use crate::SHOULD_RESUME;
    use core::sync::atomic::Ordering;
    use num_traits::ToPrimitive;

    const SYSTEM_CLOCK_FREQUENCY: u32 = 100_000_000;
    const SYSTEM_TICK_INTERVAL_MS: u32 = 100;

    fn timer_tick(_irq_no: usize, arg: *mut usize) {
        let mut timer = CSR::new(arg as *mut u32);
        // this call forces pre-emption every timer tick
        // rsyscalls are "raw syscalls" -- used for syscalls that don't have a friendly wrapper around them
        // since ReturnToParent is only used here, we haven't wrapped it, so we use an rsyscall
        xous::rsyscall(xous::SysCall::ReturnToParent(xous::PID::new(1).unwrap(), 0))
            .expect("couldn't return to parent");

        // acknowledge the timer
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 0b1);
    }

    // we do all the suspend/resume coordination in an interrupt context
    // the process state is pushed to main memory prior to entering this routine, so it's staged for a resume
    // on a clean suspend then resume, the loader will queue up this interrupt to be the first thing to
    // run on Xous resume, with a bit in the RESUME register set.
    fn susres_handler(_irq_no: usize, arg: *mut usize) {
        let sr = unsafe{ &mut *(arg as *mut SusResHw) };
        // clear the pending interrupt
        sr.csr.wfo(utra::susres::EV_PENDING_SOFT_INT, 1);

        // set this to true to do a touch-and-go suspend/resume (no actual power off, but the whole prep cycle in play)
        let touch_and_go = true;
        if touch_and_go {
            sr.csr.wfo(utra::susres::STATE_RESUME, 1);
        }

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
        /// OS timer CSR
        os_timer: utralib::CSR<u32>,
        /// memory region for the "clean suspend" marker
        marker: xous::MemoryRange,
        /// loader stack region -- this data is dirtied on every resume; claim it in this process so no others accidentally use it
        loader_stack: xous::MemoryRange,
        /// backing store for the ticktimer value
        stored_time: Option<u64>,
        /// so we can access the build seed and detect if the FPGA image was changed on us
        seed_csr: utralib::CSR<u32>,
    }
    impl SusResHw {
        pub fn new() -> Self {
            // os timer initializations
            let ostimer_csr  = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::timer0::HW_TIMER0_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Ticktimer CSR range");
            xous::claim_interrupt(
                utra::timer0::TIMER0_IRQ,
                timer_tick,
                ostimer_csr.as_mut_ptr() as *mut usize,
            ).expect("couldn't claim IRQ");

            // everything else
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
            let seed_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::seed::HW_SEED_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Seed CSR range");
            let mut sr = SusResHw {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                os_timer: CSR::new(ostimer_csr.as_mut_ptr() as *mut u32),
                stored_time: None,
                marker,
                loader_stack,
                seed_csr: CSR::new(seed_csr.as_mut_ptr() as *mut u32),
            };

            // start the OS timer running
            let ms = SYSTEM_TICK_INTERVAL_MS;
            sr.os_timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
            // load its values
            sr.os_timer.wfo(utra::timer0::LOAD_LOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
            sr.os_timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
            // enable the timer
            sr.os_timer.wfo(utra::timer0::EN_EN, 0b1);

            // Set EV_ENABLE, this starts pre-emption
            sr.os_timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0b1);

            // check that the marker has been zero'd by map_memory. Once this passes we can probably get rid of this code.
            let check_marker =  marker.as_ptr() as *const [u32; 1024];
            for words in 0..1024 {
                // note to self: don't use log:: here because we're upstream of logging being initialized
                assert!(unsafe{(*check_marker)[words]} == 0, "marker had non-zero entry!");
            }
            // clear the loader stack, mostly to get rid of unused code warnings
            let stack = loader_stack.as_ptr() as *mut [u32; 1024];
            for words in 0..1024 {
                unsafe{(*stack)[words] = 0;}
            }

            xous::claim_interrupt(
                utra::susres::SUSRES_IRQ,
                susres_handler,
                (&mut sr) as *mut SusResHw as *mut usize,
            ).expect("couldn't claim IRQ");

            sr
        }

        pub fn setup_timeout_csr(&mut self, cid: xous::CID) -> Result<(), xous::Error> {
            xous::send_message(cid,
                xous::Message::new_scalar(crate::TimeoutOpcode::SetCsr.to_usize().unwrap(), self.csr.base as usize, 0, 0, 0)
            ).map(|_| ())
        }

        pub fn do_suspend(&mut self, forced: bool) {
            println!("Stopping preemption");
            // stop pre-emption
            self.os_timer.wfo(utra::timer0::EN_EN, 0);
            self.os_timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);

            // make sure we're able to handle a soft interrupt
            self.csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, 1);

            // ensure that the resume bit is not set
            self.csr.wfo(utra::susres::STATE_RESUME, 0);

            println!("Stopping ticktimer");
            // stop deferred thread scheduling, by stopping the ticktimer
            self.csr.wfo(utra::susres::CONTROL_PAUSE, 1);
            while self.csr.rf(utra::susres::STATUS_PAUSED) == 0 {
                // busy-wait until we confirm the ticktimer has paused
            }
            self.stored_time = Some(
               (self.csr.r(utra::susres::TIME0) as u64 |
               (self.csr.r(utra::susres::TIME1) as u64) << 32)
               + 0 // a placeholder in case we need to advance time on save
            );
            println!("Stored time: {}", self.stored_time.unwrap());

            // setup the clean suspend marker, note if things were forced
            const WORDS_PER_PAGE: usize = 1024;
            let marker: *mut[u32; WORDS_PER_PAGE] = self.marker.as_mut_ptr() as *mut[u32; 1024];

            // get some entropy from the kernel using a special syscall crafted for this purpose
            let (r0, r1, r2, r3) = xous::create_server_id().unwrap().to_u32();
            let (r4, r5, r6, r7) = xous::create_server_id().unwrap().to_u32();
            const RANGES: usize = 8;
            let entropy: [u32; RANGES] = [r0, r1, r2, r3, r4, r5, r6, r7];
            /* now stripe the entropy into the clean suspend marker to create a structure that is
               likely to be corrupted if power is lost even for a short while, and also unlikely to
               be reproduced by accident

               The general structure is:
               - 8x 128-word ranges = 1024 words = 1 page
               - The first 127 words are one of four fixed word patterns selected by indexing through
                 a random word
               - EXCEPT for the 0th range, the first word is 0 if the suspend was not forced; then the next
                 64 bits (2 words) are the build seed of the current FPGA
                 The loader will check this seed on the next boot, so if the FPGA image changed it's a clean boot
               - The 128th word is a murmur3 hash of the previous 127 words

               Rationale:
               - we use 8x murmur32 hashes to reduce the chance of collisions (effectively 256-bit hash space)
               - we don't just fill the memory with random numbers because we don't want to exhaust the TRNG
                 pool during a suspend: generating more entropy takes a lot of time. Generally, the kernel
                 wil always have at least a few words of entropy on hand, though.
               - we "stretch out" our random numbers by mapping them into word patterns that are compliments
                 that have perhaps some chance to exercise the array structure of the RAM on power-up
               - we include the build seed so that for many use cases we don't accidentally resume from a suspend
                 built for a different FPGA image. This fails in the case that someone has fixed the seed for
                 the purpose of reproducible builds.
               - we hash a whole page instead of just checking a couple words because power-off bit corruption
                 on the RAM will be accumulated faster with a larger sampling size.
               - we don't hash the entire physical RAM space because we can't, due to process isolation enforced
                 by the virtual memory system. But that would be a more robust way to do this, if we made some
                 mechanism to reach around all the protections. But...a mechanism that can reach around all protections
                 can reach around all protections, and is probably not worth the risk of abuse. So just checking a page of RAM
                 is probably a reasonable compromise between security and robustness of detecting a short power-down.
            */
            let seed0 = self.seed_csr.r(utra::seed::SEED0);
            let seed1 = self.seed_csr.r(utra::seed::SEED1);
            let range = WORDS_PER_PAGE / RANGES;
            let mut index: usize = 0;
            for &e in entropy.iter() {
                for i in 0..(range - 1) {
                    let word = match (e >> ((i % 4) * 2)) & 0x3 {
                        0 => 0x0000_0000,
                        1 => 0xAA33_33AA,
                        2 => 0xFFFF_FFFF,
                        3 => 0xCC55_55CC,
                        _ => 0x3141_5923, // this should really never happen, but Rust wants it
                    };
                    unsafe{(*marker)[index + i] = word};
                }
                if index == 0 {
                    if !forced {
                        unsafe{(*marker)[index + 0] = 0};
                    } else {
                        unsafe{(*marker)[index + 0] = 1};
                    }
                    unsafe{(*marker)[index + 1] = seed0};
                    unsafe{(*marker)[index + 2] = seed1};
                }
                let mut hashbuf: [u32; WORDS_PER_PAGE / RANGES - 1] = [0; WORDS_PER_PAGE / RANGES - 1];
                for i in 0..hashbuf.len() {
                    hashbuf[i] = unsafe{(*marker)[index + i]};
                }
                let hash = murmur3_32( &hashbuf, 0);
                unsafe{(*marker)[index + range - 1] = hash;}
                println!("Clean suspend hash: {:03} <- 0x{:08x}", index + range - 1, hash);
                index += range;
            }


            // clear the loader stack, for no particular reason other than to be vengeful.
            let stack = self.loader_stack.as_ptr() as *mut [u32; 1024];
            for words in 0..1024 {
                unsafe{(*stack)[words] = 0;}
            }

            println!("Triggering suspend interrupt");
            // trigger an interrupt to process the final suspend bits
            self.csr.wfo(utra::susres::INTERRUPT_INTERRUPT, 1);

            // SHOULD_RESUME will be set true by the interrupt context when it re-enters from resume
            while !SHOULD_RESUME.load(Ordering::Relaxed) {
                println!("Waiting for resume");
                xous::yield_slice();
            }
        }
        pub fn do_resume(&mut self) -> bool { // returns true if the previous suspend was forced
            // resume the ticktimer where it left off
            println!("Trying to resume");
            if let Some(time)= self.stored_time.take() {
                // zero out the clean-suspend marker
                let marker: *mut [u32; 1024] = self.marker.as_mut_ptr() as *mut[u32; 1024];
                for words in 0..1024 {
                    unsafe{(*marker)[words] = 0x0;}
                }

                // restore the ticktimer
                self.csr.wfo(utra::susres::CONTROL_PAUSE, 1); // ensure that the ticktimer is paused before we try to load it
                self.csr.wo(utra::susres::RESUME_TIME0, time as u32);
                self.csr.wo(utra::susres::RESUME_TIME1, (time >> 32) as u32);
                // load the saved value -- can only be done while the timer is paused
                self.csr.wo(utra::susres::CONTROL,
                    self.csr.ms(utra::susres::CONTROL_PAUSE, 1) |
                    self.csr.ms(utra::susres::CONTROL_LOAD, 1)
                );
                println!("Ticktimer loaded");

                // set up pre-emption timer
                let ms = SYSTEM_TICK_INTERVAL_MS;
                self.os_timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
                // load its values
                self.os_timer.wfo(utra::timer0::LOAD_LOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
                self.os_timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
                // clear any pending interrupts so we have some time to exit
                self.os_timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
                // Set EV_ENABLE, this starts pre-emption
                self.os_timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0b1);
                // enable the pre-emption timer
                self.os_timer.wfo(utra::timer0::EN_EN, 0b1);

                // start the tickttimer running
                self.csr.wo(utra::susres::CONTROL, 0);
                println!("Ticktimer and OS timer now running");

                // clear the loader stack, for no other reason other than to be vengeful
                let stack = self.loader_stack.as_ptr() as *mut [u32; 1024];
                for words in 0..1024 {
                    unsafe{(*stack)[words] = 0;}
                }
            } else {
                panic!("Can't resume because the ticktimer value was not saved properly before suspend!")
            };

            if self.csr.rf(utra::susres::STATE_WAS_FORCED) == 0 {
                false
            } else {
                true
            }
        }
    }

}

#[cfg(not(target_os = "none"))]
mod implementation {
    use num_traits::ToPrimitive;

    pub struct SusResHw {
    }
    impl SusResHw {
        pub fn new() -> Self {
            SusResHw {}
        }
        pub fn do_suspend(&mut self, _forced: bool) {
        }
        pub fn do_resume(&mut self) -> bool {
            false
        }
        pub fn setup_timeout_csr(&mut self, cid: xous::CID) -> Result<(), xous::Error> {
            xous::send_message(cid,
                xous::Message::new_scalar(crate::TimeoutOpcode::SetCsr.to_usize().unwrap(), 0, 0, 0, 0)
            ).map(|_| ())
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
    unsafe{xous::disconnect(TIMEOUT_CONN.load(Ordering::Relaxed)).unwrap()};
    TIMEOUT_CONN.store(0, Ordering::Relaxed);
    xous::destroy_server(sid).unwrap();
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
                println!("execution gated!");
                while !RESUME_EXEC.load(Ordering::Relaxed) {
                    println!("execution gate active");
                    xous::yield_slice();
                }
                println!("execution is ungated!");
                xous::return_scalar(msg.sender, 0).expect("couldn't return dummy message to unblock execution");
            }),
            Some(ExecGateOpcode::Drop) => {
                break;
            }
            None => {
                log::error!("received unknown opcode in execution_gate");
            }
        }
    }
    xous::destroy_server(execgate_sid).unwrap();
}

#[xous::xous_main]
fn xmain() -> ! {
    // Start the OS timer which is responsible for setting up preemption.
    // os_timer::init();
    let mut susres_hw = implementation::SusResHw::new();

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    // these only print if the "debugprint" feature is specified in Cargo.toml
    // they're necessary because we kill IPC and thread comms as part of suspend
    // so this is the only way to debug what's going on.
    println!("App UART debug printing is on!");

    // start up the execution gate
    xous::create_thread_0(execution_gate).unwrap();

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
    susres_hw.setup_timeout_csr(timeout_outgoing_conn).expect("couldn't set hardware CSR for timeout thread");

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
                log::trace!("suspendready with token {}", token);
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
                    suspend_subscribers[token] = Some(scb);

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
                        log::trace!("all callbacks reporting in, doing suspend");
                        timeout_pending = false;
                        susres_hw.do_suspend(false);
                        // when do_suspend() returns, it means we've resumed
                        suspend_requested = false;
                        if susres_hw.do_resume() {
                            log::error!("We did a clean shut-down, but bootloader is saying previous suspend was forced. Some peripherals may be in an unclean state!");
                        }
                        // this now allows all other threads to commence
                        log::trace!("low-level resume done, restoring execution");
                        RESUME_EXEC.store(true, Ordering::Relaxed);
                    } else {
                        log::trace!("still waiting on callbacks, returning to main loop");
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
                    log::trace!("suspend call has timed out, forcing a suspend");
                    timeout_pending = false;
                    // force a suspend
                    susres_hw.do_suspend(true);
                    // when do_suspend() returns, it means we've resumed
                    suspend_requested = false;
                    if susres_hw.do_resume() {
                        log::error!("We forced a suspend, some peripherals may be in an unclean state!");
                    } else {
                        log::error!("We forced a suspend, but the bootloader is claiming we did a clean suspend. Internal state may be inconsistent.");
                    }
                    RESUME_EXEC.store(true, Ordering::Relaxed);
                } else {
                    log::trace!("clean suspend timeout received, ignoring");
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
