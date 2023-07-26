#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#![recursion_limit="512"]

mod api;
use api::*;

use num_traits::*;
use core::sync::atomic::{AtomicBool, Ordering};
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;

#[cfg(any(feature="precursor", feature="renode"))]
#[macro_use]
extern crate engine25519_as;

static RUN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static DISALLOW_SUSPEND: AtomicBool = AtomicBool::new(false);
static SUSPEND_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

#[cfg(any(feature="precursor", feature="renode"))]
mod implementation {
    use utralib::generated::*;
    use crate::api::*;
    use susres::{RegManager, RegOrField, SuspendResume};
    use num_traits::*;
    use core::sync::atomic::Ordering;
    use crate::RUN_IN_PROGRESS;
    use crate::DISALLOW_SUSPEND;
    use core::convert::TryInto;

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
        montgomery_len: Option<usize>,
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
            Engine25519Hw {
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
                montgomery_len: None,
            }
        }

        pub fn init(&mut self) {
            log::trace!("claiming interrupt");
            xous::claim_interrupt(
                utra::engine::ENGINE_IRQ,
                handle_engine_irq,
                self as *mut Engine25519Hw as *mut usize,
            )
            .expect("couldn't claim engine irq");

            log::trace!("enabling interrupt");
            self.csr.wo(utra::engine::EV_PENDING, 0xFFFF_FFFF); // clear any droppings.
            self.csr.wo(utra::engine::EV_ENABLE,
                self.csr.ms(utra::engine::EV_ENABLE_FINISHED, 1) |
                self.csr.ms(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, 1)
            );

            // setup the susres context. Most of the defaults are fine, so they aren't explicitly initialized in the code above,
            // but they still have to show up down here in case we're suspended mid-op
            log::trace!("setting up susres");
            self.susres.push(RegOrField::Reg(utra::engine::POWER), None); // on resume, this needs to be setup first, so that the "pause" state is captured correctly
            self.susres.push(RegOrField::Reg(utra::engine::WINDOW), None);
            self.susres.push(RegOrField::Reg(utra::engine::MPSTART), None);
            self.susres.push(RegOrField::Reg(utra::engine::MPLEN), None);
            self.susres.push_fixed_value(RegOrField::Reg(utra::engine::EV_PENDING), 0xFFFF_FFFF);

            // manually handle event enable, because the bootloader could have messed with our current state
            // self.susres.push(RegOrField::Reg(utra::engine::EV_ENABLE), None);

            // self.susres.push(RegOrField::Reg(utra::engine::CONTROL), None); // don't push this, we need to manually coordinate `mpcresume` before resuming
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
            // disable interrupts
            self.csr.wo(utra::engine::EV_ENABLE, 0);

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

            log::info!("orig_state: {:?}", orig_state);
            // clear any droppings from the bootloader, and then re-enable
            self.csr.wo(utra::engine::EV_PENDING, 0xFFFF_FFFF);
            self.csr.wo(utra::engine::EV_ENABLE,
                self.csr.ms(utra::engine::EV_ENABLE_FINISHED, 1) |
                self.csr.ms(utra::engine::EV_ENABLE_ILLEGAL_OPCODE, 1)
            );

            // in the case of a resume from pause, we need to specify the PC to resume from
            // clear the pause
            if let Some(mpc) = self.mpc_resume {
                log::info!("suspended during engine25519 transaction, resuming...");
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
                log::info!("engine25519 simple resume");
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
            self.montgomery_len = None;
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
        fn copy_reg(&mut self, r: [u8; 32], ra: usize, window: usize) {
            for (src, dst) in r.chunks_exact(4).zip(self.rf_hw[window * RF_SIZE_IN_U32 + ra * 8..window * RF_SIZE_IN_U32 + (ra+1) * 8].iter_mut()) {
                unsafe{ (dst as *mut u32).write_volatile(u32::from_le_bytes(src[0..4].try_into().unwrap()));}
            }
        }
        fn load_montgomery(&mut self, mpstart: u32) -> u32 {
            let mcode = assemble_engine25519!(
                start:
                    // P.U in %20
                    // P.W in %21
                    // Q.U in %22
                    // Q.W in %23
                    // affine_PmQ in %24
                    // %30 is the TRD scratch register and cswap dummy
                    // %29 is the subtraction temporary value register and k_t
                    // x0.U in %25
                    // x0.W in %26
                    // x1.U in %27
                    // x1.W in %28
                    // %19 is the loop counter, starts with 254 (if 0, loop runs exactly once)
                    // %31 is the scalar
                    // %18 is the swap variable
                    psa %18, #0

                    // for i in (0..255).rev()
                mainloop:
                    // let choice: u8 = (bits[i + 1] ^ bits[i]) as u8;
                    // ProjectivePoint::conditional_swap(&mut x0, &mut x1, choice.into());
                    xbt %29, %31        // orignally[k_t = (k>>t) & 1] now[k_t = k[254]]
                    shl %31, %31        // k = k<<1
                    xor %18, %18, %29   // swap ^= k_t

                    // cswap x0.U (%25), x1.U (%27)
                    xor %30, %25, %27
                    msk %30, %18, %30
                    xor %25, %30, %25
                    xor %27, %30, %27
                    // cswap x0.W (%26), x1.W (%28)
                    xor %30, %26, %28
                    msk %30, %18, %30
                    xor %26, %30, %26
                    xor %28, %30, %28

                    psa %18, %29  // swap = k_t

                        // differential_add_and_double(&mut x0, &mut x1, &affine_u);
                        psa %20, %25
                        psa %21, %26
                        psa %22, %27
                        psa %23, %28
                        // affine_u is already in %24

                        // let t0 = &P.U + &P.W;
                        add %0, %20, %21
                        trd %30, %0
                        sub %0, %0, %30
                        // let t1 = &P.U - &P.W;
                        sub %21, #3, %21    // negate &P.W using #FIELDPRIME (#3)
                        add %1, %20, %21
                        trd %30, %1
                        sub %1, %1, %30
                        // let t2 = &Q.U + &Q.W;
                        add %2, %22, %23
                        trd %30, %2
                        sub %2, %2, %30
                        // let t3 = &Q.U - &Q.W;
                        sub %23, #3, %23
                        add %3, %22, %23
                        trd %30, %3
                        sub %3, %3, %30
                        // let t4 = t0.square();   // (U_P + W_P)^2 = U_P^2 + 2 U_P W_P + W_P^2
                        mul %4, %0, %0
                        // let t5 = t1.square();   // (U_P - W_P)^2 = U_P^2 - 2 U_P W_P + W_P^2
                        mul %5, %1, %1
                        // let t6 = &t4 - &t5;     // 4 U_P W_P
                        sub %29, #3, %5
                        add %6, %4, %29
                        trd %30, %6
                        sub %6, %6, %30
                        // let t7 = &t0 * &t3;     // (U_P + W_P) (U_Q - W_Q) = U_P U_Q + W_P U_Q - U_P W_Q - W_P W_Q
                        mul %7, %0, %3
                        // let t8 = &t1 * &t2;     // (U_P - W_P) (U_Q + W_Q) = U_P U_Q - W_P U_Q + U_P W_Q - W_P W_Q
                        mul %8, %1, %2
                        // let t9  = &t7 + &t8;    // 2 (U_P U_Q - W_P W_Q)
                        add %9, %7, %8
                        trd %30, %9
                        sub %9, %9, %30
                        // let t10 = &t7 - &t8;    // 2 (W_P U_Q - U_P W_Q)
                        sub %29, #3, %8
                        add %10, %7, %29
                        trd %30, %10
                        sub %10, %10, %30
                        // let t11 =  t9.square(); // 4 (U_P U_Q - W_P W_Q)^2
                        mul %11, %9, %9
                        // let t12 = t10.square(); // 4 (W_P U_Q - U_P W_Q)^2
                        mul %12, %10, %10
                        // let t13 = &APLUS2_OVER_FOUR * &t6; // (A + 2) U_P U_Q
                        mul %13, #4, %6   // #4 is A+2/4
                        // let t14 = &t4 * &t5;    // ((U_P + W_P)(U_P - W_P))^2 = (U_P^2 - W_P^2)^2
                        mul %14, %4, %5
                        // let t15 = &t13 + &t5;   // (U_P - W_P)^2 + (A + 2) U_P W_P
                        add %15, %13, %5
                        trd %30, %15
                        sub %15, %15, %30
                        // let t16 = &t6 * &t15;   // 4 (U_P W_P) ((U_P - W_P)^2 + (A + 2) U_P W_P)
                        mul %16, %6, %15
                        // let t17 = affine_PmQ * &t12; // U_D * 4 (W_P U_Q - U_P W_Q)^2
                        mul %17, %24, %12    // affine_PmQ loaded into %24

                        ///// these can be eliminated down the road, but included for 1:1 algorithm correspodence to reference in early testing
                        // P.U = t14;  // U_{P'} = (U_P + W_P)^2 (U_P - W_P)^2
                        psa %20, %14
                        // P.W = t16;  // W_{P'} = (4 U_P W_P) ((U_P - W_P)^2 + ((A + 2)/4) 4 U_P W_P)
                        psa %21, %16
                        // let t18 = t11;               // W_D * 4 (U_P U_Q - W_P W_Q)^2
                        // Q.U = t18;  // U_{Q'} = W_D * 4 (U_P U_Q - W_P W_Q)^2
                        psa %22, %11   // collapsed two to save a register
                        // Q.W = t17;  // W_{Q'} = U_D * 4 (W_P U_Q - U_P W_Q)^2
                        psa %23, %17

                        ///// 'return' arguments for next iteration, can be optimized out later
                        psa %25, %20
                        psa %26, %21
                        psa %27, %22
                        psa %28, %23

                    brz end, %19     // if loop counter is 0, quit
                    sub %19, %19, #1 // subtract one from the loop counter and run again
                    brz mainloop, #0    // go back to the top
                end:
                    // ProjectivePoint::conditional_swap(&mut x0, &mut x1, Choice::from(bits[0] as u8));
                    // cswap x0.U (%25), x1.U (%27)
                    xor %30, %25, %27
                    msk %30, %18, %30
                    xor %25, %30, %25
                    xor %27, %30, %27
                    // cswap x0.W (%26), x1.W (%28)
                    xor %30, %26, %28
                    msk %30, %18, %30
                    xor %26, %30, %26
                    xor %28, %30, %28

                    // AFFINE SPLICE -- pass arguments to the affine block
                    psa %29, %25
                    psa %30, %26
                    // W.invert() in %21
                    // U in %29
                    // W in %30
                    // result in %31
                    // loop counter in %28

                    // from FieldElement.invert()
                        // let (t19, t3) = self.pow22501();   // t19: 249..0 ; t3: 3,1,0
                        // let t0  = self.square();           // 1         e_0 = 2^1
                        mul %0, %30, %30  // self is W, e.g. %30
                        // let t1  = t0.square().square();    // 3         e_1 = 2^3
                        mul %1, %0, %0
                        mul %1, %1, %1
                        // let t2  = self * &t1;              // 3,0       e_2 = 2^3 + 2^0
                        mul %2, %30, %1
                        // let t3  = &t0 * &t2;               // 3,1,0
                        mul %3, %0, %2
                        // let t4  = t3.square();             // 4,2,1
                        mul %4, %3, %3
                        // let t5  = &t2 * &t4;               // 4,3,2,1,0
                        mul %5, %2, %4

                        // let t6  = t5.pow2k(5);             // 9,8,7,6,5
                        psa %28, #5       // coincidentally, constant #5 is the number 5
                        mul %6, %5, %5
                    pow2k_5:
                        sub %28, %28, #1  // %28 = %28 - 1
                        brz pow2k_5_exit, %28
                        mul %6, %6, %6
                        brz pow2k_5, #0
                    pow2k_5_exit:
                        // let t7  = &t6 * &t5;               // 9,8,7,6,5,4,3,2,1,0
                        mul %7, %6, %5

                        // let t8  = t7.pow2k(10);            // 19..10
                        psa %28, #6        // constant #6 is the number 10
                        mul %8, %7, %7
                    pow2k_10:
                        sub %28, %28, #1
                        brz pow2k_10_exit, %28
                        mul %8, %8, %8
                        brz pow2k_10, #0
                    pow2k_10_exit:
                        // let t9  = &t8 * &t7;               // 19..0
                        mul %9, %8, %7

                        // let t10 = t9.pow2k(20);            // 39..20
                        psa %28, #7         // constant #7 is the number 20
                        mul %10, %9, %9
                    pow2k_20:
                        sub %28, %28, #1
                        brz pow2k_20_exit, %28
                        mul %10, %10, %10
                        brz pow2k_20, #0
                    pow2k_20_exit:
                        // let t11 = &t10 * &t9;              // 39..0
                        mul %11, %10, %9

                        // let t12 = t11.pow2k(10);           // 49..10
                        psa %28, #6         // constant #6 is the number 10
                        mul %12, %11, %11
                    pow2k_10b:
                        sub %28, %28, #1
                        brz pow2k_10b_exit, %28
                        mul %12, %12, %12
                        brz pow2k_10b, #0
                    pow2k_10b_exit:
                        // let t13 = &t12 * &t7;              // 49..0
                        mul %13, %12, %7

                        // let t14 = t13.pow2k(50);           // 99..50
                        psa %28, #8         // constant #8 is the number 50
                        mul %14, %13, %13
                    pow2k_50a:
                        sub %28, %28, #1
                        brz pow2k_50a_exit, %28
                        mul %14, %14, %14
                        brz pow2k_50a, #0
                    pow2k_50a_exit:
                        // let t15 = &t14 * &t13;             // 99..0
                        mul %15, %14, %13

                        // let t16 = t15.pow2k(100);          // 199..100
                        psa %28, #9         // constant #9 is the number 100
                        mul %16, %15, %15
                    pow2k_100:
                        sub %28, %28, #1
                        brz pow2k_100_exit, %28
                        mul %16, %16, %16
                        brz pow2k_100, #0
                    pow2k_100_exit:
                        // let t17 = &t16 * &t15;             // 199..0
                        mul %17, %16, %15

                        // let t18 = t17.pow2k(50);           // 249..50
                        psa %28, #8         // constant #8 is the number 50
                        mul %18, %17, %17
                    pow2k_50b:
                        sub %28, %28, #1
                        brz pow2k_50b_exit, %28
                        mul %18, %18, %18
                        brz pow2k_50b, #0
                    pow2k_50b_exit:
                        // let t19 = &t18 * &t13;             // 249..0
                        mul %19, %18, %13
                        //(t19, t3) // just a return value, values are already there, do nothing

                        //let t20 = t19.pow2k(5);            // 254..5
                        psa %28, #5
                        mul %20, %19, %19
                    pow2k_5_last:
                        sub %28, %28, #1
                        brz pow2k_5_last_exit, %28
                        mul %20, %20, %20
                        brz pow2k_5_last, #0
                    pow2k_5_last_exit:

                        //let t21 = &t20 * &t3;              // 254..5,3,1,0
                        mul %21, %20, %3

                    // u = &self.U * &self.W.invert()
                    mul %31, %29, %21
                    fin  // finish execution
            );
            for (&src, dst) in mcode.iter().zip(self.ucode_hw[mpstart as usize..].iter_mut()) {
                unsafe { (dst as *mut u32).write_volatile(src as u32) };
            }
            mcode.len() as u32
        }
        pub fn montgomery(&mut self, job: MontgomeryJob) {
            log::trace!("entering run");
            // block any suspends from happening while we set up the engine
            DISALLOW_SUSPEND.store(true, Ordering::Relaxed);

            let window: usize = 0;

            self.copy_reg(job.x0_u, 25, window);
            self.copy_reg(job.x0_w, 26, window);
            self.copy_reg(job.x1_u, 27, window);
            self.copy_reg(job.x1_w, 28, window);
            self.copy_reg(job.affine_u, 24, window);
            self.copy_reg(job.scalar, 31, window);
            self.copy_reg([
                254, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
               0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
               0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
               0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ], 19, window); // 254 as loop counter

            let mpstart = 0;
            // this optimization shaves off about 0.13ms per iteration
            if self.montgomery_len.is_none() {
                self.montgomery_len = Some(self.load_montgomery(mpstart) as usize);
            }
            self.csr.wfo(utra::engine::WINDOW_WINDOW, window as u32); // this value should now be validated because an invalid window would cause a panic on slice copy
            self.csr.wfo(utra::engine::MPSTART_MPSTART, mpstart);
            self.csr.wfo(utra::engine::MPLEN_MPLEN, self.montgomery_len.unwrap() as u32);

            log::trace!("sanity check uc{:08x}, rf{:08x}", self.ucode_hw[0], self.rf_hw[0]);

            // sync calls poll a state variable, and thus no message is sent
            self.do_notify = false;

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
        pub fn get_single_result(&mut self, r: usize) -> JobResult {
            if let Some(clean_resume) = self.clean_resume {
                if !clean_resume {
                    return JobResult::SuspendError;
                }
            }
            if self.illegal_opcode {
                return JobResult::IllegalOpcodeException;
            }

            let mut ret_r: [u8; 32] = [0; 32];
            let window = self.csr.rf(utra::engine::WINDOW_WINDOW) as usize;
            for (&src, dst) in self.rf_hw[window * RF_SIZE_IN_U32 + r * 8..window * RF_SIZE_IN_U32 + (r+1) * 8].iter().zip(ret_r.chunks_exact_mut(4)) {
                for (&src_byte, dst_byte) in src.to_le_bytes().iter().zip(dst.iter_mut()) {
                    *dst_byte = src_byte;
                }
            }

            JobResult::SingleResult(ret_r)
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use crate::api::*;

    pub struct Engine25519Hw {
    }

    impl Engine25519Hw {
        pub fn new(_handler_conn: xous::CID) -> Engine25519Hw {
            Engine25519Hw {
            }
        }
        pub fn init(&mut self) {}
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
        pub fn run(&mut self, _job: Job) {
        }
        pub fn get_result(&mut self) -> JobResult {
            JobResult::IllegalOpcodeException
        }
        pub fn power_on(&mut self, _on: bool) {
        }
        pub fn montgomery(&mut self, _job: MontgomeryJob) {
        }
        pub fn get_single_result(&mut self, _r: usize) -> JobResult {
            JobResult::EngineUnavailable
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
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, api::SusResOps::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

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

fn main() -> ! {
    use crate::implementation::Engine25519Hw;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let engine25519_sid = xns.register_name(api::SERVER_NAME_ENGINE25519, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", engine25519_sid);

    let handler_conn = xous::connect(engine25519_sid).expect("couldn't create IRQ handler connection");
    log::trace!("creating engine25519 object");
    let mut engine25519 = Box::new(Engine25519Hw::new(handler_conn));
    engine25519.init();

    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    xous::create_thread_1(susres_thread, engine25519.as_mut() as *mut Engine25519Hw as usize).expect("couldn't start susres handler thread");

    let mut client_cid: Option<xous::CID> = None;
    let mut job_count = 0;
    let mut mont_count = 0;
    loop {
        let mut msg = xous::receive_message(engine25519_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::MontgomeryJob) => {
                if mont_count % 100 == 0 {
                    log::info!("montgomery job {}", mont_count); // leave this here for now so we can confirm that HW acceleration is being selected when we think it is!
                }
                mont_count += 1;
                // don't start a new job if a suspend is in progress
                while SUSPEND_IN_PROGRESS.load(Ordering::Relaxed) {
                    log::trace!("waiting for suspend to finish");
                    xous::yield_slice();
                }
                engine25519.power_on(true);
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let montgomery_job = buffer.to_original::<MontgomeryJob, _>().unwrap();
                engine25519.montgomery(montgomery_job);
                while RUN_IN_PROGRESS.load(Ordering::Relaxed) {
                    // block until the job is done
                    xous::yield_slice();
                }
                let result = engine25519.get_single_result(31); // return the result
                engine25519.power_on(false);
                buffer.replace(result).unwrap();
            }
            Some(Opcode::RunJob) => {
                if job_count % 100 == 0 {
                    log::info!("engine job {}", job_count); // leave this here for now so we can confirm that HW acceleration is being selected when we think it is!
                }
                job_count += 1;
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
