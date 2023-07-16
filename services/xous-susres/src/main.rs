#![cfg_attr(not(target_os = "none"), allow(dead_code))]

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod murmur3;

use xous_api_susres::*;

use num_traits::{ToPrimitive, FromPrimitive};
use xous_ipc::Buffer;
use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack, send_message, Message};
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use xous::messages::sender::Sender;

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


#[cfg(any(feature="precursor", feature="renode"))]
mod implementation {
    use utralib::generated::*;
    use crate::murmur3::murmur3_32;
    use crate::SHOULD_RESUME;
    use core::sync::atomic::Ordering;
    use num_traits::ToPrimitive;

    const SYSTEM_CLOCK_FREQUENCY: u32 = 12_000_000; // timer0 is now in the always-on domain
    const SYSTEM_TICK_INTERVAL_MS: u32 = xous::BASE_QUANTA_MS;

    fn timer_tick(_irq_no: usize, arg: *mut usize) {
        let mut timer = CSR::new(arg as *mut u32);
        // this call forces preemption every timer tick
        // rsyscalls are "raw syscalls" -- used for syscalls that don't have a friendly wrapper around them
        // since ReturnToParent is only used here, we haven't wrapped it, so we use an rsyscall
        xous::rsyscall(xous::SysCall::ReturnToParent(xous::PID::new(1).unwrap(), 0))
            .expect("couldn't return to parent");

        // acknowledge the timer
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 0b1);
    }

    #[cfg(feature = "sus_reboot")]
    static REBOOT_CSR: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    // we do all the suspend/resume coordination in an interrupt context
    // the process state is pushed to main memory prior to entering this routine, so it's staged for a resume
    // on a clean suspend then resume, the loader will queue up this interrupt to be the first thing to
    // run on Xous resume, with a bit in the RESUME register set.
    fn susres_handler(_irq_no: usize, arg: *mut usize) {
        //println!("susres handler");
        let sr = unsafe{ &mut *(arg as *mut SusResHw) };
        // clear the pending interrupt
        sr.csr.wfo(utra::susres::EV_PENDING_SOFT_INT, 1);

        // set this to true to do a touch-and-go suspend/resume (no actual power off, but the whole prep cycle in play)
        let touch_and_go = false;
        if touch_and_go {
            sr.csr.wfo(utra::susres::STATE_RESUME, 1);
        }

        if sr.csr.rf(utra::susres::STATE_RESUME) == 0 {
            //println!("going into suspend");
            #[cfg(feature = "sus_reboot")]
            { // this is just for testing, doing a quick full-soc boot instead of a power down
                let mut reboot_csr = CSR::new(REBOOT_CSR.load(Ordering::Relaxed) as *mut u32);
                reboot_csr.wfo(utra::reboot::SOC_RESET_SOC_RESET, 0xAC);
            }

            // flush the L2 cache by writing 0's to a region of memory that matches the L2 cache size
            if let Some(cf) = sr.cacheflush {
                let cf_ptr = cf.as_ptr() as *mut u32;
                for i in 0..(cf.len() / 4) {
                    unsafe {cf_ptr.add(i).write_volatile(0xacdc_acdc); }
                }
            }

            // prevent re-ordering
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            // power the system down - this should result in an almost immediate loss of power
            loop {
                sr.csr.wfo(utra::susres::POWERDOWN_POWERDOWN, 1);
            } // block forever here
        } else {
            //println!("going into resume");
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
        /// backing store for the ticktimer value
        stored_time: Option<u64>,
        /// so we can access the build seed and detect if the FPGA image was changed on us
        seed_csr: utralib::CSR<u32>,
        /// we also own the reboot facility
        reboot_csr: utralib::CSR<u32>,
        /// cache flushing memory raea
        cacheflush: Option<xous::MemoryRange>,
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

            // note that mapping a page zeroes it out. Plus, this should have been zero'd by the bootloader.
            let marker = xous::syscall::map_memory(
                xous::MemoryAddress::new(0x4100_0000 - 0x3000), // this is a special, hard-coded location; 0x2000 is the size of the bootloader's stack area
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
            let reboot_csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::reboot::HW_REBOOT_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Reboot CSR range");
            #[cfg(feature = "sus_reboot")]
            REBOOT_CSR.store(reboot_csr.as_mut_ptr() as u32, Ordering::Relaxed); // for testing only
            let mut sr = SusResHw {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                os_timer: CSR::new(ostimer_csr.as_mut_ptr() as *mut u32),
                stored_time: None,
                marker,
                seed_csr: CSR::new(seed_csr.as_mut_ptr() as *mut u32),
                reboot_csr: CSR::new(reboot_csr.as_mut_ptr() as *mut u32),
                cacheflush: None,
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

            sr
        }
        pub fn init(&mut self) {
            xous::claim_interrupt(
                utra::susres::SUSRES_IRQ,
                susres_handler,
                self as *mut SusResHw as *mut usize,
            ).expect("couldn't claim IRQ");
        }
        pub fn ignore_wfi(&mut self) {
            self.csr.wfo(utra::susres::WFI_OVERRIDE, 1);
        }
        pub fn restore_wfi(&mut self) {
            self.csr.wfo(utra::susres::WFI_OVERRIDE, 0);
        }

        pub fn reboot(&mut self, reboot_soc: bool) {
            if reboot_soc {
                self.reboot_csr.wfo(utra::reboot::SOC_RESET_SOC_RESET, 0xAC);
            } else {
                self.reboot_csr.wfo(utra::reboot::CPU_RESET_CPU_RESET, 1);
            }
        }
        pub fn set_reboot_vector(&mut self, vector: u32) {
            self.reboot_csr.wfo(utra::reboot::ADDR_ADDR, vector);
        }
        pub fn force_power_off(&mut self) {
            loop {
                self.csr.wfo(utra::susres::POWERDOWN_POWERDOWN, 1);
                xous::yield_slice();
            } // block forever here
        }
        /// Safety: this should only be called once by the main suspend/resume loop
        /// to create a copy of the timeout engine inside the timeout handler.
        pub (crate) unsafe fn setup_timeout_csr(&mut self, cid: xous::CID) -> Result<(), xous::Error> {
            xous::send_message(cid,
                xous::Message::new_scalar(
                crate::TimeoutOpcode::SetCsr.to_usize().unwrap(),
                self.csr.base() as usize,
                0, 0, 0)
            ).map(|_| ())
        }

        pub fn do_suspend(&mut self, forced: bool) {
            #[cfg(feature = "debugprint")]
            println!("Stopping preemption");
            // stop pre-emption
            self.os_timer.wfo(utra::timer0::EN_EN, 0);
            self.os_timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);

            // make sure we're able to handle a soft interrupt
            self.csr.wfo(utra::susres::EV_ENABLE_SOFT_INT, 1);

            // ensure that the resume bit is not set
            self.csr.wfo(utra::susres::STATE_RESUME, 0);

            #[cfg(feature = "debugprint")]
            println!("Stopping ticktimer");
            // stop deferred thread scheduling, by stopping the ticktimer
            self.csr.wfo(utra::susres::CONTROL_PAUSE, 1);
            while self.csr.rf(utra::susres::STATUS_PAUSED) == 0 {
                // busy-wait until we confirm the ticktimer has paused
            }
            self.stored_time = Some(
               (self.csr.r(utra::susres::TIME0) as u64 |
               (self.csr.r(utra::susres::TIME1) as u64) << 32)
               + 1 // a placeholder in case we need to advance time on save
            );
            #[cfg(feature = "debugprint")]
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
            let pid = xous::process::id() as u32;
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
                    unsafe{(*marker)[index + 3] = pid};
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

            // allocate memory for the cache flush
            if self.cacheflush.is_none() {
                self.cacheflush = Some (
                    xous::syscall::map_memory(
                        None,
                        None,
                        // L2 cache is 128k, but it takes 512k to flush it, because the L1 D$ has 4 ways
                        // If for some reason we can't allocate 512k we can re-write this to use
                        // unsafe { core::arch::asm!(".word 0x500F"); }
                        // to flush the L1 cache after every 4k of data written (one way size)
                        // however, the memory is de-allocated on resume, so this isn't
                        // a permanent penalty to pay, and changing this could would require
                        // a validation cycle that I don't want to go through right now.
                        512 * 1024,
                        xous::MemoryFlags::R | xous::MemoryFlags::W | xous::MemoryFlags::RESERVE,
                    ).expect("couldn't allocate RAM for cache flushing")
                );
                // the RESERVE flag should pre-allocate the pages, but for good measure
                // we write all the pages to make sure they are at a defined value
                if let Some(cf) = self.cacheflush {
                    let cf_ptr = cf.as_ptr() as *mut u32;
                    for i in 0..(cf.len() / 4) {
                        unsafe {cf_ptr.add(i).write_volatile(0x0bad_0bad); }
                    }
                }
            }

            #[cfg(feature = "debugprint")]
            println!("Triggering suspend interrupt");
            // trigger an interrupt to process the final suspend bits
            self.csr.wfo(utra::susres::INTERRUPT_INTERRUPT, 1);

            // SHOULD_RESUME will be set true by the interrupt context when it re-enters from resume
            while !SHOULD_RESUME.load(Ordering::Relaxed) {
                #[cfg(feature = "debugprint")]
                println!("Waiting for resume");
                xous::yield_slice();
            }
        }
        pub fn do_resume(&mut self) -> bool { // returns true if the previous suspend was forced
            // resume the ticktimer where it left off
            #[cfg(feature = "debugprint")]
            println!("Trying to resume");
            if let Some(time)= self.stored_time.take() {
                // zero out the clean-suspend marker
                let marker: *mut [u32; 1024] = self.marker.as_mut_ptr() as *mut[u32; 1024];
                for words in 0..1024 {
                    unsafe{(*marker)[words] = 0x0;}
                }

                // restore the ticktimer
                self.csr.wo(utra::susres::RESUME_TIME0, time as u32);
                self.csr.wo(utra::susres::RESUME_TIME1, (time >> 32) as u32);
                #[cfg(feature = "debugprint")]
                println!("Ticktimer loaded with {}", time);

                // set up pre-emption timer
                let ms = SYSTEM_TICK_INTERVAL_MS;
                self.os_timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
                // load its values
                self.os_timer.wfo(utra::timer0::LOAD_LOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
                self.os_timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
                // clear any pending interrupts so we have some time to exit
                self.os_timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
                // Set EV_ENABLE, this starts preemption
                self.os_timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0b1);
                // enable the preemption timer
                self.os_timer.wfo(utra::timer0::EN_EN, 0b1);

                // start the ticktimer running
                self.csr.wo(utra::susres::CONTROL,
                    self.csr.ms(utra::susres::CONTROL_LOAD, 1)
                );
                log::trace!("Resume {} / control {}", self.csr.r(utra::susres::RESUME_TIME0), self.csr.r(utra::susres::CONTROL));
                log::trace!("Ticktimer loaded with {} / {}", time, self.csr.r(utra::susres::TIME0));
                self.csr.wo(utra::susres::CONTROL, 0);
                #[cfg(feature = "debugprint")]
                println!("Ticktimer and OS timer now running");

            } else {
                panic!("Can't resume because the ticktimer value was not saved properly before suspend!")
            };

            // de-allocate the cache flush memory
            if let Some(cf) = self.cacheflush.take() {
                xous::syscall::unmap_memory(cf).expect("couldn't de-allocate cache flush region");
            }

            if self.csr.rf(utra::susres::STATE_WAS_FORCED) == 0 {
                false
            } else {
                true
            }
        }
        fn get_hw_time(&self) -> u64 {
            self.csr.r(utra::susres::TIME0) as u64 | ((self.csr.r(utra::susres::TIME1) as u64) << 32)
        }
        pub fn debug_delay(&self, duration: u32) {
            let start = self.get_hw_time();
            while ((self.get_hw_time() - start) as u32) < duration {
                xous::yield_slice();
            }
        }
    }

}

#[cfg(any(not(target_os = "xous"),
    not(any(feature="precursor", feature="renode", not(target_os = "xous"))) // default for crates.io
))]
mod implementation {
    use num_traits::ToPrimitive;

    pub struct SusResHw {
    }
    impl SusResHw {
        pub fn new() -> Self {
            SusResHw {}
        }
        pub fn reboot(&self, _reboot_soc: bool) {}
        pub fn set_reboot_vector(&self, _vector: u32) {}
        pub fn force_power_off(&mut self) {}
        pub fn do_suspend(&mut self, _forced: bool) {
        }
        pub fn do_resume(&mut self) -> bool {
            false
        }
        pub (crate) unsafe fn setup_timeout_csr(&mut self, cid: xous::CID) -> Result<(), xous::Error> {
            xous::send_message(cid,
                xous::Message::new_scalar(crate::TimeoutOpcode::SetCsr.to_usize().unwrap(), 0, 0, 0, 0)
            ).map(|_| ())
        }
        pub fn ignore_wfi(&mut self) {}
        pub fn restore_wfi(&mut self) {}
        pub fn debug_delay(&self, _duration: u32) {}
    }
}

#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
    ready_to_suspend: bool,
    token: u32,
    failed_to_suspend: bool,
    order: xous_api_susres::api::SuspendOrder,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
enum TimeoutOpcode {
    SetCsr,
    Run,
    Drop,
}

static TIMEOUT_TIME: AtomicU32 = AtomicU32::new(5000); // this is gated by the possibility that an EC reset was called just as a suspend was initiated. EC reset takes about 3500ms
static TIMEOUT_CONN: AtomicU32 = AtomicU32::new(0);
pub fn timeout_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    #[cfg(any(feature="precursor", feature="renode"))]
    use utralib::generated::*;
    #[cfg(any(feature="precursor", feature="renode"))]
    let mut csr: Option<CSR::<u32>> = None;
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            #[cfg(any(feature="precursor", feature="renode"))]
            Some(TimeoutOpcode::SetCsr) => msg_scalar_unpack!(msg, base, _, _, _, {
                csr = Some(CSR::new(base as *mut u32));
            }),
            #[cfg(any(not(target_os = "xous"),
              not(any(feature="precursor", feature="renode", not(target_os = "xous"))) // default for crates.io
            ))]
            Some(TimeoutOpcode::SetCsr) => msg_scalar_unpack!(msg, _base, _, _, _, {
                // ignore the opcode in hosted mode
            }),
            Some(TimeoutOpcode::Run) => {
                #[cfg(any(feature="precursor", feature="renode"))]
                {
                    // we have to re-implement the ticktimer time reading here because as we wait for the timeout,
                    // the ticktimer goes away! so we use the susres local copy with direct hardware ops to keep track of time in this phase
                    fn get_hw_time(hw: CSR::<u32>) -> u64 {
                        hw.r(utra::susres::TIME0) as u64 | ((hw.r(utra::susres::TIME1) as u64) << 32)
                    }
                    if let Some(hw) = csr {
                        let start = get_hw_time(hw);
                        let timeout = TIMEOUT_TIME.load(Ordering::Relaxed); // ignore updates to timeout once we're waiting
                        while ((get_hw_time(hw) - start) as u32) < timeout {
                            // log::info!("delta t: {}", (get_hw_time(hw) - start) as u32);
                            xous::yield_slice();
                        }
                    } else {
                        panic!("hardware CSR not sent to timeout_thread before it was instructed to run");
                    }
                }
                log::trace!("HW timeout reached");
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

fn main() -> ! {
    // Start the OS timer which is responsible for setting up preemption.
    // os_timer::init();
    let mut susres_hw = Box::new(implementation::SusResHw::new());
    susres_hw.init();

    log_server::init_wait().unwrap();
    // debugging note: it's actually easiest to debug using client-side hooks. Search for
    // "<-- use this to debug s/r" in the lib.rs file and switch that to an "info" level
    // and you will get a nice readout of caller PID + token lists.
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    // these only print if the "debugprint" feature is specified in Cargo.toml
    // they're necessary because we kill IPC and thread comms as part of suspend
    // so this is the only way to debug what's going on.
    println!("App UART debug printing is on!");

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed
    let susres_sid = xns.register_name(api::SERVER_NAME_SUSRES, None).expect("can't register server");
    log::trace!("main loop registered with NS -- {:?}", susres_sid);

    // make a connection for the timeout thread to wake us up
    let timeout_incoming_conn = xous::connect(susres_sid).unwrap();
    TIMEOUT_CONN.store(timeout_incoming_conn, Ordering::Relaxed);
    // allocate a private server ID for the timeout thread, it's not registered with the name server
    let timeout_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = timeout_sid.to_u32();
    xous::create_thread_4(timeout_thread, sid0 as usize, sid1 as usize, sid2 as usize, sid3 as usize).expect("couldn't create timeout thread");
    let timeout_outgoing_conn = xous::connect(timeout_sid).expect("couldn't connect to our timeout thread");
    // safety: we are cloning a CSR and handing to another thread that is coded to only
    // operate on the registers disjoint from those used by the rest of the code (therefore
    // no stomping on values).
    unsafe{
        susres_hw.setup_timeout_csr(timeout_outgoing_conn).expect("couldn't set hardware CSR for timeout thread");
    }

    let mut suspend_requested: Option<Sender> = None;
    let mut timeout_pending = false;
    let mut reboot_requested: bool = false;
    let mut allow_suspend = true;

    let mut suspend_subscribers = Vec::<ScalarCallback>::new();
    let mut current_op_order = crate::api::SuspendOrder::Early;

    let mut gated_pids = Vec::<xous::MessageSender>::new();
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        if reboot_requested {
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::RebootCpuConfirm) => {
                    susres_hw.reboot(false);
                }
                Some(Opcode::RebootSocConfirm) => {
                    susres_hw.reboot(true);
                }
                _ => reboot_requested = false,
            }
        } else {
            match FromPrimitive::from_usize(msg.body.id()) {
                Some(Opcode::RebootRequest) => {
                    reboot_requested = true;
                },
                Some(Opcode::RebootCpuConfirm) => {
                    log::error!("RebootCpuConfirm, but no prior Request. Ignoring.");
                },
                Some(Opcode::RebootSocConfirm) => {
                    log::error!("RebootSocConfirm, but no prior Request. Ignoring.");
                },
                Some(Opcode::RebootVector) =>  msg_scalar_unpack!(msg, vector, _, _, _, {
                    susres_hw.set_reboot_vector(vector as u32);
                }),
                Some(Opcode::SuspendEventSubscribe) => {
                    let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                    let hookdata = buffer.to_original::<ScalarHook, _>().unwrap();
                    do_hook(hookdata, &mut suspend_subscribers);
                },
                Some(Opcode::SuspendingNow) => {
                    if suspend_requested.is_none() {
                        // this is harmless, it means a process' execution gate came a bit later than expected, so just ignore and tell it to resume
                        // the execution gate is only requested until *after* a process has checked in and said it is ready to suspend, anyways.
                        log::warn!("exec gate message received late from pid {:?}, ignoring", msg.sender.pid());
                        xous::return_scalar(msg.sender, 0).expect("couldn't return dummy message to unblock execution");
                    } else {
                        gated_pids.push(msg.sender);
                    }
                },
                Some(Opcode::SuspendReady) => msg_scalar_unpack!(msg, token, _, _, _, {
                    log::debug!("SuspendReady with token {}", token);
                    if suspend_requested.is_none() {
                        log::error!("received a SuspendReady message when a suspend wasn't pending from token {}", token);
                        continue;
                    }
                    if token >= suspend_subscribers.len() {
                        panic!("received a SuspendReady token that's out of range");
                    }
                    let scb = &mut suspend_subscribers[token];
                    if scb.ready_to_suspend {
                        log::error!("received a duplicate SuspendReady token: {} from {:?}", token, scb);
                        continue;
                    }
                    scb.ready_to_suspend = true;

                    // DEBUG NOTES:
                    // "<-- use this to debug s/r" in the lib.rs file and switch that to an "info" level
                    // in order to get the best-quality debug info (lib-side hook can give us caller PID)

                    // also, note that llio's suspend call will map out the UART on suspend. If you want to
                    // debug a kernel panic on resume, you must set the mux to 0, but then you lose
                    // visibility on suspend once the llio triggers its suspend. If you want to debug suspend
                    // order problems, set the mux to 1, but you lose visibility into KP on resume.
                    let mut all_ready = true;
                    for sub in suspend_subscribers.iter() {
                        if sub.order == current_op_order {
                            if !sub.ready_to_suspend {
                                log::debug!("  -> NOT READY token: {}", sub.token);
                                all_ready = false;
                                break;
                            }
                        }
                    }
                    // note: we must have at least one `Last` subscriber for this logic to work!
                    if all_ready && current_op_order == crate::api::SuspendOrder::Last {
                        log::info!("all callbacks reporting in, doing suspend");
                        timeout_pending = false;
                        // susres_hw.debug_delay(500); // let the messages print
                        susres_hw.do_suspend(false);

                        // ---- power turns off ----
                        // ---- time passes while we are off. The FPGA is powered off; all registers are lost, but RAM is retained. ----
                        // ---- omg power came back! ---

                        // when do_suspend() returns, it means we've resumed
                        let sender = suspend_requested.take().expect("suspend was requested, but no requestor is on record!");

                        log_server::resume(); // log server is a special case, in order to avoid circular dependencies
                        if susres_hw.do_resume() {
                            log::error!("We did a clean shut-down, but bootloader is saying previous suspend was forced. Some peripherals may be in an unclean state!");
                        }
                        // this now allows all other threads to commence
                        log::trace!("low-level resume done, restoring execution");
                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0).expect("couldn't return dummy message to unblock execution");
                        }
                        susres_hw.restore_wfi();

                        // this unblocks the requestor of the suspend
                        xous::return_scalar(sender, 1).ok();
                    } else if all_ready {
                        log::debug!("finished with {:?} going to next round", current_op_order);
                        // the current order is finished, send the next tranche
                        current_op_order = current_op_order.next();
                        let mut at_least_one_event_sent = false;
                        while !at_least_one_event_sent {
                            let (send_success, next_op_order) = send_event(&suspend_subscribers, current_op_order);
                            if !send_success {
                                current_op_order = next_op_order;
                            }
                            at_least_one_event_sent = send_success;
                        }
                        log::debug!("Now waiting on {:?} stage", current_op_order);
                        // let the events fire
                        xous::yield_slice();
                    } else {
                        log::trace!("still waiting on callbacks, returning to main loop");
                    }
                }),
                Some(Opcode::SuspendRequest) => {
                    /*
                    log::info!("registered suspend listeners:");
                    for sub in suspend_subscribers.iter() {
                        log::info!("{:?}", sub);
                    }*/
                    // if the 2-second timeout is still pending from a previous suspend, deny the suspend request.
                    // ...just don't suspend that quickly after resuming???
                    if allow_suspend && !timeout_pending {
                        susres_hw.ignore_wfi();
                        suspend_requested = Some(msg.sender);
                        // clear the resume gate
                        SHOULD_RESUME.store(false, Ordering::Relaxed);
                        // clear the ready to suspend flag and failed to suspend flag
                        for sub in suspend_subscribers.iter_mut() {
                            sub.ready_to_suspend = false;
                            sub.failed_to_suspend = false;
                        }
                        // do we want to start the timeout before or after sending the notifications? hmm. 🤔
                        timeout_pending = true;
                        send_message(timeout_outgoing_conn,
                            Message::new_scalar(TimeoutOpcode::Run.to_usize().unwrap(), 0, 0, 0, 0)
                        ).expect("couldn't initiate timeout before suspend!");

                        current_op_order = crate::api::SuspendOrder::Early;
                        let mut at_least_one_event_sent = false;
                        while !at_least_one_event_sent {
                            let (send_success, next_op_order) = send_event(&suspend_subscribers, current_op_order);
                            if !send_success {
                                current_op_order = next_op_order;
                            }
                            at_least_one_event_sent = send_success;
                        }
                        // let the events fire
                        xous::yield_slice();
                    } else {
                        log::warn!("suspend requested, but the system was not allowed to suspend. Ignoring request.");
                        xous::return_scalar(msg.sender, 0).ok();
                    }
                },
                Some(Opcode::SuspendTimeout) => {
                    if timeout_pending {
                        // record which tokens had not reported in
                        for sub in suspend_subscribers.iter_mut() {
                            sub.failed_to_suspend = !sub.ready_to_suspend;
                        }
                        timeout_pending = false;
                        log::warn!("Suspend timed out, forcing an unclean suspend at stage {:?}", current_op_order);
                        for sub in suspend_subscribers.iter() {
                            if sub.order == current_op_order {
                                if !sub.ready_to_suspend {
                                    // note to debugger: you will get a token number, which is in itself not useful.
                                    // There should be, at least once in the debug log, printed on the very first suspend cycle,
                                    // a list of PID->tokens. Tokens are assigned in the order that the registration happens
                                    // to the susres server. Empirically, this list is generally stable for every build,
                                    // and is guaranteed to be stable across a single cold boot.
                                    log::warn!("  -> NOT READY TOKEN: {}", sub.token);
                                }
                            }
                        }
                        // In case of timeout, skip the suspend cycle and return a failure, instead of forcing the suspend.
                        /*
                        susres_hw.debug_delay(500); // let the messages print
                        // force a suspend
                        susres_hw.do_suspend(true);

                        // ---- power turns off ----
                        // ---- time passes while we are asleep ----
                        // ---- omg power came back! ---

                        log_server::resume(); // log server is a special case, in order to avoid circular dependencies
                        if susres_hw.do_resume() {
                            log::error!("We forced a suspend, some peripherals may be in an unclean state!");
                        } else {
                            log::error!("We forced a suspend, but the bootloader is claiming we did a clean suspend. Internal state may be inconsistent.");
                        }
                        */
                        let sender = suspend_requested.take().expect("suspend was requested, but no requestor is on record!");
                        for pid in gated_pids.drain(..) {
                            xous::return_scalar(pid, 0).expect("couldn't return dummy message to unblock execution");
                        }
                        susres_hw.restore_wfi();

                        // this unblocks the requestor of the suspend
                        xous::return_scalar(sender, 0).ok();
                    } else {
                        log::trace!("clean suspend timeout received, ignoring");
                        // this means we did a clean suspend, we've resumed, and the timeout came back after the resume
                        // just ignore the message.
                    }
                }
                Some(Opcode::WasSuspendClean) => msg_blocking_scalar_unpack!(msg, token, _, _, _, {
                    let mut clean = true;
                    for sub in suspend_subscribers.iter() {
                        if sub.token == token as u32 && sub.failed_to_suspend {
                            clean = false;
                        }
                    }
                    if clean {
                        xous::return_scalar(msg.sender, 1).expect("couldn't return WasSuspendClean result");
                    } else {
                        xous::return_scalar(msg.sender, 0).expect("couldn't return WasSuspendClean result");
                    }
                }),
                Some(Opcode::SuspendAllow) => {
                    allow_suspend = true;
                },
                Some(Opcode::SuspendDeny) => {
                    allow_suspend = false;
                },
                Some(Opcode::PowerOff) => {
                    susres_hw.force_power_off();
                }
                Some(Opcode::Quit) => {
                    break
                }
                None => {
                    log::error!("couldn't convert opcode");
                }
            }
        }
    }
    // clean up our program
    unhook(&mut suspend_subscribers);
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(susres_sid).unwrap();
    xous::destroy_server(susres_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut Vec::<ScalarCallback>) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
        ready_to_suspend: false,
        token: cb_conns.len() as u32,
        failed_to_suspend: false,
        order: hookdata.order,
    };
    log::trace!("hooking {:?}", cb_dat);
    cb_conns.push(cb_dat);
}
fn unhook(cb_conns: &mut Vec::<ScalarCallback>) {
    for scb in cb_conns.iter() {
        xous::send_message(scb.server_to_cb_cid,
            xous::Message::new_blocking_scalar(SuspendEventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)
        ).unwrap();
        unsafe{xous::disconnect(scb.server_to_cb_cid).unwrap();}
    }
    cb_conns.clear();
}
fn send_event(cb_conns: &Vec::<ScalarCallback>, order: crate::api::SuspendOrder) -> (bool, crate::api::SuspendOrder) {
    let mut at_least_one_event_sent = false;
    log::info!("Sending suspend to {:?} stage", order);
    /*
    // abortive attempt to get suspend to shut down the system. Doesn't work, results in a panic because too many messages are still moving around.
    #[cfg(not(target_os = "xous"))]
    {
        if order == crate::api::SuspendOrder::Last {
            let tt_conn = xous::connect(xous::SID::from_bytes(b"ticktimer-server").unwrap()).unwrap();
            send_message(
                tt_conn,
                xous::Message::new_blocking_scalar(
                    1,
                    1000,
                    0,
                    0,
                    0,
                ),
            )
            .map(|_| ()).unwrap();
            xous::rsyscall(xous::SysCall::Shutdown).expect("unable to quit");
        }
    }*/
    for scb in cb_conns.iter() {
        if scb.order == order {
            at_least_one_event_sent = true;
            xous::send_message(scb.server_to_cb_cid,
                xous::Message::new_scalar(SuspendEventCallback::Event.to_usize().unwrap(),
                scb.cb_to_client_cid as usize, scb.cb_to_client_id as usize, scb.token as usize, 0)
            ).unwrap();
        }
    }
    (at_least_one_event_sent, order.next())
}
