#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::*;
use xous::CID;
use xous_ipc::Buffer;

use log::info;

// when we are testing, allocate a fairly large chunk of data, because the overhead of switching and conditioning
// the TRNG into the test mode is fairly high
const TEST_CACHE_LEN: usize = 128 * 1024;
#[derive(Copy, Clone, Debug)]
struct ScalarCallback {
    server_to_cb_cid: CID,
    cb_to_client_cid: CID,
    cb_to_client_id: u32,
}

#[cfg(any(feature="precursor", feature="renode"))]
mod implementation {
    use crate::api::{ExcursionTest, HealthTests, MiniRunsTest, NistTests, TrngBuf, TrngErrors, TrngTestMode, TRNG_TEST_BUF_LEN, TrngTestBuf};
    use crate::TEST_CACHE_LEN;
    use num_traits::*;
    use susres::{RegManager, RegOrField, SuspendResume};
    use utralib::generated::*;

    pub struct Trng {
        csr: utralib::CSR<u32>,
        susres_manager: RegManager<{ utra::trng_server::TRNG_SERVER_NUMREGS }>, // probably can be reduced to save space?
        conn: xous::CID,
        errors: TrngErrors,
        err_stat: HealthTests,
        last_test_mode: TrngTestMode,
    }

    fn trng_handler(_irq_no: usize, arg: *mut usize) {
        let trng = unsafe { &mut *(arg as *mut Trng) };
        // cache a copy of the stats in the interrupt handler, so we can diagnose later
        trng.err_stat = trng.get_tests();

        let pending = trng.csr.r(utra::trng_server::EV_PENDING);
        trng.errors.pending_mask = pending;
        if (pending & trng.csr.ms(utra::trng_server::EV_PENDING_EXCURSION0, 1)) != 0 {
            trng.errors.excursion_errs[0] = Some(ExcursionTest {
                min: trng.csr.rf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MIN) as u16,
                max: trng.csr.rf(utra::trng_server::AV_EXCURSION0_LAST_ERR_MAX) as u16,
            });
            trng.csr
                .rmwf(utra::trng_server::AV_EXCURSION0_CTRL_RESET, 1);
        }
        if (pending & trng.csr.ms(utra::trng_server::EV_PENDING_EXCURSION1, 1)) != 0 {
            trng.errors.excursion_errs[1] = Some(ExcursionTest {
                min: trng.csr.rf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MIN) as u16,
                max: trng.csr.rf(utra::trng_server::AV_EXCURSION1_LAST_ERR_MAX) as u16,
            });
            trng.csr
                .rmwf(utra::trng_server::AV_EXCURSION1_CTRL_RESET, 1);
        }
        if (pending & trng.csr.ms(utra::trng_server::EV_PENDING_HEALTH, 1)) != 0 {
            let av_repcount = trng.csr.rf(utra::trng_server::NIST_ERRORS_AV_REPCOUNT);
            let av_adaptive = trng.csr.rf(utra::trng_server::NIST_ERRORS_AV_ADAPTIVE);
            let ro_repcount = trng.csr.rf(utra::trng_server::NIST_ERRORS_RO_REPCOUNT);
            let ro_adaptive = trng.csr.rf(utra::trng_server::NIST_ERRORS_RO_ADAPTIVE);
            if av_repcount != 0 {
                trng.errors.av_repcount_errs = Some(av_repcount as u8);
            }
            if av_adaptive != 0 {
                trng.errors.av_adaptive_errs = Some(av_adaptive as u8);
            }
            if ro_repcount != 0 {
                trng.errors.ro_repcount_errs = Some(ro_repcount as u8);
            }
            if ro_adaptive != 0 {
                trng.errors.ro_adaptive_errs = Some(ro_adaptive as u8);
            }
        }
        // record error summaries and errors from non-health sources
        trng.errors.nist_errs = trng.csr.r(utra::trng_server::NIST_ERRORS);
        trng.errors.server_underruns =
            trng.csr.rf(utra::trng_server::UNDERRUNS_SERVER_UNDERRUN) as u16;
        trng.errors.kernel_underruns =
            trng.csr.rf(utra::trng_server::UNDERRUNS_KERNEL_UNDERRUN) as u16;

        // reset any error flags. try to do this a bit away from the pending clear, so it has time to take effect
        trng.csr.rmwf(utra::trng_server::CONTROL_CLR_ERR, 1);

        // notify the main loop of the error condition
        xous::try_send_message(
            trng.conn,
            xous::Message::new_scalar(
                crate::api::Opcode::ErrorNotification.to_usize().unwrap(),
                0,
                0,
                0,
                0,
            ),
        )
        .map(|_| ())
        .unwrap();

        // clear the pending interrupt(s)
        trng.csr.wo(utra::trng_server::EV_PENDING, pending);
    }

    impl Trng {
        pub fn new(xns: &xous_names::XousNames) -> Trng {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng_server::HW_TRNG_SERVER_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG CSR range");

            let mut trng = Trng {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                conn: xns
                    .request_connection_blocking(crate::api::SERVER_NAME_TRNG)
                    .unwrap(),
                susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
                errors: TrngErrors {
                    excursion_errs: [None; 2],
                    av_repcount_errs: None,
                    av_adaptive_errs: None,
                    ro_repcount_errs: None,
                    ro_adaptive_errs: None,
                    kernel_underruns: 0,
                    server_underruns: 0,
                    nist_errs: 0,
                    pending_mask: 0,
                },
                err_stat: HealthTests::default(),
                last_test_mode: TrngTestMode::None,
            };

            trng.csr.wo(
                utra::trng_server::CONTROL,
                trng.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1),
            );
            log::trace!(
                "TRNG configured for normal operation (av+ro): 0x{:08x}",
                trng.csr.r(utra::trng_server::CONTROL)
            );

            trng.susres_manager
                .push(RegOrField::Reg(utra::trng_server::CONTROL), None);

            /*** TRNG tuning parameters: these were configured and tested in a long run against Dieharder
                There is a rate of TRNG generation vs. quality trade-off. The tuning below is toward quality of
                TRNG versus rate of TRNG, such that we could use these without any whitening.
            ***/
            ///// configure avalanche
            // delay in microseconds for avalanche poweron after powersave
            trng.csr.wo(
                utra::trng_server::AV_CONFIG,
                trng.csr.ms(utra::trng_server::AV_CONFIG_POWERDELAY, 50_000)
                    | trng.csr.ms(utra::trng_server::AV_CONFIG_SAMPLES, 32),
            );
            trng.susres_manager
                .push(RegOrField::Reg(utra::trng_server::AV_CONFIG), None);

            ///// configure ring oscillator
            trng.csr.wo(
                utra::trng_server::RO_CONFIG,
                trng.csr.ms(utra::trng_server::RO_CONFIG_DELAY, 4)
                    | trng.csr.ms(utra::trng_server::RO_CONFIG_DWELL, 100)
                    | trng.csr.ms(utra::trng_server::RO_CONFIG_GANG, 1)
                    | trng.csr.ms(utra::trng_server::RO_CONFIG_FUZZ, 1)
                    | trng.csr.ms(utra::trng_server::RO_CONFIG_OVERSAMPLING, 3),
            );
            trng.susres_manager
                .push(RegOrField::Reg(utra::trng_server::RO_CONFIG), None);

            // slightly reduce the frequency of these to reduce power
            trng.csr.wo(
                utra::trng_server::CHACHA,
                trng.csr.ms(utra::trng_server::CHACHA_RESEED_INTERVAL, 2)
                    | trng
                        .csr
                        .ms(utra::trng_server::CHACHA_SELFMIX_INTERVAL, 1000)
                    | trng.csr.ms(utra::trng_server::CHACHA_SELFMIX_ENA, 1),
            );
            trng.susres_manager
                .push(RegOrField::Reg(utra::trng_server::CHACHA), None);

            // handle error interrupts
            xous::claim_interrupt(
                utra::trng_server::TRNG_SERVER_IRQ,
                trng_handler,
                (&mut trng) as *mut Trng as *mut usize,
            )
            .expect("couldn't claim audio irq");
            trng.csr.wo(
                utra::trng_server::EV_PENDING,
                trng.csr.ms(utra::trng_server::EV_PENDING_ERROR, 1)
                    | trng.csr.ms(utra::trng_server::EV_PENDING_HEALTH, 1)
                    | trng.csr.ms(utra::trng_server::EV_PENDING_EXCURSION0, 1)
                    | trng.csr.ms(utra::trng_server::EV_PENDING_EXCURSION1, 1),
            );
            trng.csr.wo(
                utra::trng_server::EV_ENABLE,
                trng.csr.ms(utra::trng_server::EV_ENABLE_ERROR, 1)
                    | trng.csr.ms(utra::trng_server::EV_ENABLE_HEALTH, 1)
                    | trng.csr.ms(utra::trng_server::EV_ENABLE_EXCURSION0, 1)
                    | trng.csr.ms(utra::trng_server::EV_ENABLE_EXCURSION1, 1),
            );
            trng.susres_manager
                .push_fixed_value(RegOrField::Reg(utra::trng_server::EV_PENDING), 0xFFFF_FFFF);
            trng.susres_manager
                .push(RegOrField::Reg(utra::trng_server::EV_ENABLE), None);

            log::debug!("hardware initialized");

            if trng.csr.rf(utra::trng_server::STATUS_CHACHA_READY) == 0 {
                log::trace!("chacha not ready");
            } else {
                log::trace!("chacha ready");
                if trng.csr.rf(utra::trng_server::URANDOM_VALID_URANDOM_VALID) == 0 {
                    log::trace!("chacha not valid");
                }
            }
            log::trace!(
                "chacha rands: 0x{:08x} 0x{:08x} 0x{:08x} 0x{:08x}",
                trng.csr.rf(utra::trng_server::URANDOM_URANDOM),
                trng.csr.rf(utra::trng_server::URANDOM_URANDOM),
                trng.csr.rf(utra::trng_server::URANDOM_URANDOM),
                trng.csr.rf(utra::trng_server::URANDOM_URANDOM),
            );

            trng
        }

        pub fn get_errors(&self) -> TrngErrors {
            self.errors
        }
        pub fn get_err_stats(&self) -> HealthTests {
            self.err_stat
        }

        #[rustfmt::skip]
        pub fn get_tests(&self) -> HealthTests {
            // the fresh bit gets reset on the first read of the register. Ensure these reads happen first before the structure is initialized.
            let av_nist_fresh0 = if self.csr.rf(utra::trng_server::NIST_AV_STAT0_FRESH) == 0 {false} else {true};
            let av_nist_fresh1 = if self.csr.rf(utra::trng_server::NIST_AV_STAT1_FRESH) == 0 {false} else {true};
            let ro_mr_fresh0 = if self.csr.rf(utra::trng_server::RO_RUN0_FRESH_RO_RUN0_FRESH) == 0 {false} else {true};
            let ro_mr_fresh1 = if self.csr.rf(utra::trng_server::RO_RUN1_FRESH_RO_RUN1_FRESH) == 0 {false} else {true};
            let ro_mr_fresh2 = if self.csr.rf(utra::trng_server::RO_RUN2_FRESH_RO_RUN2_FRESH) == 0 {false} else {true};
            let ro_mr_fresh3 = if self.csr.rf(utra::trng_server::RO_RUN3_FRESH_RO_RUN3_FRESH) == 0 {false} else {true};
            let ro_nist_fresh0 = if self.csr.rf(utra::trng_server::NIST_RO_STAT0_FRESH) == 0 {false} else {true};
            let ro_nist_fresh1 = if self.csr.rf(utra::trng_server::NIST_RO_STAT1_FRESH) == 0 {false} else {true};
            let ro_nist_fresh2 = if self.csr.rf(utra::trng_server::NIST_RO_STAT2_FRESH) == 0 {false} else {true};
            let ro_nist_fresh3 = if self.csr.rf(utra::trng_server::NIST_RO_STAT3_FRESH) == 0 {false} else {true};
            // now initialize the return structure
            HealthTests {
                av_excursion: [
                    ExcursionTest {
                        min: self.csr.rf(utra::trng_server::AV_EXCURSION0_STAT_MIN) as u16,
                        max: self.csr.rf(utra::trng_server::AV_EXCURSION0_STAT_MAX) as u16,
                    },
                    ExcursionTest {
                        min: self.csr.rf(utra::trng_server::AV_EXCURSION1_STAT_MIN) as u16,
                        max: self.csr.rf(utra::trng_server::AV_EXCURSION1_STAT_MAX) as u16,
                    },
                ],
                av_nist: [
                    NistTests {
                        fresh: av_nist_fresh0,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_AV_STAT0_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_AV_STAT0_REP_B) as u16,
                    },
                    NistTests {
                        fresh: av_nist_fresh1,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_AV_STAT1_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_AV_STAT1_REP_B) as u16,
                    },
                ],
                ro_miniruns: [
                    MiniRunsTest {
                        fresh: ro_mr_fresh0,
                        run_count: [
                            self.csr.r(utra::trng_server::RO_RUN0_COUNT1) as u16,
                            self.csr.r(utra::trng_server::RO_RUN0_COUNT2) as u16,
                            self.csr.r(utra::trng_server::RO_RUN0_COUNT3) as u16,
                            self.csr.r(utra::trng_server::RO_RUN0_COUNT4) as u16,
                            //self.csr.r(utra::trng_server::RO_RUN0_COUNT5) as u16,
                        ],
                    },
                    MiniRunsTest {
                        fresh: ro_mr_fresh1,
                        run_count: [
                            self.csr.r(utra::trng_server::RO_RUN1_COUNT1) as u16,
                            self.csr.r(utra::trng_server::RO_RUN1_COUNT2) as u16,
                            self.csr.r(utra::trng_server::RO_RUN1_COUNT3) as u16,
                            self.csr.r(utra::trng_server::RO_RUN1_COUNT4) as u16,
                            //self.csr.r(utra::trng_server::RO_RUN1_COUNT5) as u16,
                        ],
                    },
                    MiniRunsTest {
                        fresh: ro_mr_fresh2,
                        run_count: [
                            self.csr.r(utra::trng_server::RO_RUN2_COUNT1) as u16,
                            self.csr.r(utra::trng_server::RO_RUN2_COUNT2) as u16,
                            self.csr.r(utra::trng_server::RO_RUN2_COUNT3) as u16,
                            self.csr.r(utra::trng_server::RO_RUN2_COUNT4) as u16,
                            //self.csr.r(utra::trng_server::RO_RUN2_COUNT5) as u16,
                        ],
                    },
                    MiniRunsTest {
                        fresh: ro_mr_fresh3,
                        run_count: [
                            self.csr.r(utra::trng_server::RO_RUN3_COUNT1) as u16,
                            self.csr.r(utra::trng_server::RO_RUN3_COUNT2) as u16,
                            self.csr.r(utra::trng_server::RO_RUN3_COUNT3) as u16,
                            self.csr.r(utra::trng_server::RO_RUN3_COUNT4) as u16,
                            //self.csr.r(utra::trng_server::RO_RUN3_COUNT5) as u16,
                        ],
                    },
                ],
                ro_nist: [
                    NistTests {
                        fresh: ro_nist_fresh0,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_RO_STAT0_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_RO_STAT0_REP_B) as u16,
                    },
                    NistTests {
                        fresh: ro_nist_fresh1,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_REP_B) as u16,
                    },
                    NistTests {
                        fresh: ro_nist_fresh2,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_REP_B) as u16,
                    },
                    NistTests {
                        fresh: ro_nist_fresh3,
                        adaptive_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_ADAP_B) as u16,
                        repcount_b: self.csr.rf(utra::trng_server::NIST_RO_STAT1_REP_B) as u16,
                    },
                ],
            }
        }

        pub fn get_data_eager(&mut self) -> u32 {
            let mut timeout = 0;
            if false {
                // raw random
                while self.csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
                    if timeout > 100 {
                        log::debug!(
                            "TRNG ran out of data, blocked on READY: 0x{:x}",
                            self.csr.r(utra::trng_server::READY)
                        );
                        log::debug!(
                            "ROstats: 0x{:x} 0x{:x} 0x{:x} 0x{:x}",
                            self.csr.r(utra::trng_server::NIST_RO_STAT0),
                            self.csr.r(utra::trng_server::NIST_RO_STAT1),
                            self.csr.r(utra::trng_server::NIST_RO_STAT2),
                            self.csr.r(utra::trng_server::NIST_RO_STAT3)
                        );
                        self.csr.rmwf(utra::trng_server::CONTROL_CLR_ERR, 1);
                        timeout = 0;
                    }
                    xous::yield_slice();
                    timeout += 1;
                }
                self.csr.rf(utra::trng_server::DATA_DATA)
            } else {
                // urandom
                // in practice, urandom generates data fast enough that we could skip this check
                // you would need a fully unrolled read loop to exceed the generation rate
                // but, better safe than sorry!
                while self.csr.rf(utra::trng_server::URANDOM_VALID_URANDOM_VALID) == 0 {}
                self.csr.rf(utra::trng_server::URANDOM_URANDOM)
            }
        }

        #[allow(dead_code)]
        pub fn wait_full(&self) {
            while self.csr.rf(utra::trng_server::STATUS_FULL) == 0 {
                xous::yield_slice();
            }
        }

        pub fn get_buf(&mut self, len: u16) -> TrngBuf {
            let mut tb = TrngBuf {
                data: [0; 1024],
                len,
            };
            for i in 0..len as usize {
                tb.data[i] = self.get_data_eager();
            }
            tb
        }

        pub fn get_trng(&mut self, count: usize) -> [u32; 2] {
            let mut ret: [u32; 2] = [0, 0];

            // eventually this will come from a hardware 'urandom' style interface
            // for now, we just take data directly from the hardware-managed raw TRNG pool
            ret[0] = self.get_data_eager();
            // we don't just draw down TRNGs if not requested, because they are a finite resource
            if count > 1 {
                ret[1] = self.get_data_eager();
            }

            ret
        }

        pub fn suspend(&mut self) {
            self.susres_manager.suspend();
        }
        pub fn resume(&mut self) {
            self.susres_manager.resume();
            // pump the engine to discard the initial 0's in the execution pipeline
            self.get_trng(2);
        }

        fn flush_trng_fifo(&mut self) {
            // The hardware FIFO Is 1024 entries deep x 32 bits wide. This is a hard-coded parameter derived from the trng_managed.py source code (line 1014)
            for _ in 0..1024 {
                while self.csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
                    xous::yield_slice();
                }
                let _ = self.csr.rf(utra::trng_server::DATA_DATA);
            }
        }
        /// This routine will "always" return a full buffer; if it is not full, that should be considered an error condition.
        pub fn get_trng_testmode(&mut self, test_data: &mut Vec::<u8>, test_mode: TrngTestMode) -> TrngTestBuf {
            let mut retbuf = TrngTestBuf {
                data: [0u8; TRNG_TEST_BUF_LEN],
                len: 0
            };
            // bail if the test mode isn't set
            if test_mode == TrngTestMode::None {
                return retbuf;
            }
            if self.last_test_mode != test_mode {
                // the test_data buffer is forfeit; clear it
                test_data.clear();
                self.last_test_mode = test_mode;
            }
            if test_data.len() >= TRNG_TEST_BUF_LEN {
                retbuf.data.copy_from_slice(test_data.drain(..TRNG_TEST_BUF_LEN).as_slice());
                retbuf.len = TRNG_TEST_BUF_LEN as u16;
                retbuf
            } else {
                match test_mode {
                    TrngTestMode::Av => {
                        // avalanche test only
                        self.csr.wo(
                            utra::trng_server::CONTROL,
                            self.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                            | self.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                            | self.csr.ms(utra::trng_server::CONTROL_RO_DIS, 1), // disable the RO to characterize only the AV
                        );
                        self.flush_trng_fifo();
                    }
                    TrngTestMode::Ro => {
                        // ring osc test only
                        self.csr.wo(
                            utra::trng_server::CONTROL,
                            self.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                                | self.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                                | self.csr.ms(utra::trng_server::CONTROL_AV_DIS, 1), // disable the AV generator to characterize the RO
                        );
                        self.flush_trng_fifo();
                    }
                    TrngTestMode::Both | TrngTestMode::Cprng => {
                        assert!(self.csr.rf(utra::trng_server::CONTROL_RO_DIS) == 0, "TRNG test mode inconsistency (RO reads as disabled; expected it to be enabled)");
                        assert!(self.csr.rf(utra::trng_server::CONTROL_AV_DIS) == 0, "TRNG test mode inconsistency (AV reads as disabled; expected it to be enabled)");
                    }
                    TrngTestMode::None => {
                        panic!("TRNG test buffer request with no test mode set.");
                    }
                }

                // now refill the test_data buffer
                while test_data.len() < TEST_CACHE_LEN {
                    match test_mode {
                        TrngTestMode::Av | TrngTestMode::Ro | TrngTestMode::Both => {
                            while self.csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
                                xous::yield_slice();
                            }
                            let word = self.csr.rf(utra::trng_server::DATA_DATA);
                            test_data.extend_from_slice(&word.to_le_bytes());
                        }
                        TrngTestMode::Cprng => {
                            while self.csr.rf(utra::trng_server::URANDOM_VALID_URANDOM_VALID) == 0 {
                                xous::yield_slice();
                            }
                            let word = self.csr.rf(utra::trng_server::URANDOM_URANDOM);
                            test_data.extend_from_slice(&word.to_le_bytes());
                        }
                        TrngTestMode::None => {
                            unreachable!();
                        }
                    }
                }

                // restore the normal operation mode
                match test_mode {
                    TrngTestMode::Av | TrngTestMode::Ro => {
                        // reinstate "normal" operation (both on)
                        self.csr.wo(
                            utra::trng_server::CONTROL,
                            self.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                                | self.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1),
                        );
                        self.flush_trng_fifo();
                    }
                    // the raw TRNG data already matches the expected system mode; nothing to do
                    _ => {},
                }

                // copy the test data into a return structure
                retbuf.data.copy_from_slice(test_data.drain(..TRNG_TEST_BUF_LEN).as_slice());
                retbuf.len = TRNG_TEST_BUF_LEN as u16;
                retbuf
            }
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::rand_core::RngCore;
    use crate::api::{HealthTests, TrngBuf, TrngErrors, TRNG_TEST_BUF_LEN, TrngTestBuf, TrngTestMode};

    pub struct Trng {
        rng: ChaCha8Rng,
        seed: u32,
        msgcount: u16, // re-print the message every time we rollover
    }

    impl Trng {
        pub fn new(_xns: &xous_names::XousNames) -> Trng {
            Trng {
                rng: ChaCha8Rng::seed_from_u64(xous::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst)),
                seed: 0x1afe_cafe,
                msgcount: 0,
            }
        }

        fn move_lfsr(&self, mut lfsr: u32) -> u32 {
            lfsr ^= lfsr >> 7;
            lfsr ^= lfsr << 9;
            lfsr ^= lfsr >> 13;
            lfsr
        }

        #[allow(dead_code)]
        pub fn wait_full(&self) {}

        pub fn get_buf(&mut self, len: u16) -> TrngBuf {
            if self.msgcount < 3 {
                log::info!("hosted mode TRNG is *not* random, it is a deterministic LFSR");
            }
            self.msgcount += 1;
            let mut data = [0; 1024];
            for d in data.iter_mut() {
                *d = self.rng.next_u32();
            }
            TrngBuf {
                data,
                len,
            }
        }

        pub fn get_trng(&mut self, _count: usize) -> [u32; 2] {
            if self.msgcount < 3 {
                log::info!("hosted mode TRNG is *not* random, it is a deterministic LFSR");
            }
            self.msgcount += 1;
            let mut ret: [u32; 2] = [0; 2];
            self.seed = self.move_lfsr(self.seed);
            ret[0] = self.seed;
            self.seed = self.move_lfsr(self.seed);
            ret[1] = self.seed;

            ret
        }
        pub fn suspend(&self) {}
        pub fn resume(&self) {}
        pub fn get_tests(&self) -> HealthTests {
            HealthTests::default()
        }
        pub fn get_errors(&self) -> TrngErrors {
            TrngErrors {
                excursion_errs: [None; 2],
                av_repcount_errs: None,
                av_adaptive_errs: None,
                ro_repcount_errs: None,
                ro_adaptive_errs: None,
                kernel_underruns: 0,
                server_underruns: 0,
                nist_errs: 0,
                pending_mask: 0,
            }
        }
        pub fn get_err_stats(&self) -> HealthTests {
            HealthTests::default()
        }
        pub fn get_trng_testmode(&mut self, _test_data: &mut Vec::<u8>, _test_mode: TrngTestMode) -> TrngTestBuf {
            TrngTestBuf {
                data: [0u8; TRNG_TEST_BUF_LEN],
                len: 0
            }
        }
    }
}

fn main() -> ! {
    use crate::implementation::Trng;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed, anyone including less-trusted processes can get a random number
    let trng_sid = xns
        .register_name(api::SERVER_NAME_TRNG, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", trng_sid);

    let mut trng = Trng::new(&xns);

    #[cfg(feature = "avalanchetest")]
    log::info!("TRNG built with avalanche test enabled");

    #[cfg(feature = "ringosctest")]
    log::info!("TRNG built with ring oscillator test enabled");

    #[cfg(feature = "urandomtest")]
    log::info!("TRNG built with urandom test enabled");

    #[cfg(any(
        feature = "avalanchetest",
        feature = "ringosctest",
        feature = "urandomtest"
    ))]
    xous::create_thread_1(tester_thread, trng.get_trng_csr() as usize)
        .expect("couldn't create test thread");

    // pump the TRNG hardware to clear the first number out, sometimes it is 0 due to clock-sync issues on the fifo
    trng.get_trng(2);
    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let sr_cid = xous::connect(trng_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Later), &xns, api::Opcode::SuspendResume as u32, sr_cid)
        .expect("couldn't create suspend/resume object");

    let mut error_cb_conns: [Option<ScalarCallback>; 32] = [None; 32];

    // storage for testing data. We heap-allocate it, so that it's not a burden when we aren't testing the TRNG
    let mut test_data = Vec::<u8>::new();
    let mut test_mode = TrngTestMode::None;

    loop {
        let mut msg = xous::receive_message(trng_sid).unwrap();
        let op: Option<api::Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", op);
        match op {
            Some(api::Opcode::GetTrng) => xous::msg_blocking_scalar_unpack!(msg, count, _, _, _, {
                let val: [u32; 2] = trng.get_trng(count);
                xous::return_scalar2(msg.sender, val[0] as _, val[1] as _)
                    .expect("couldn't return GetTrng request");
            }),
            Some(api::Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                trng.suspend();
                susres
                    .suspend_until_resume(token)
                    .expect("couldn't execute suspend/resume");
                trng.resume();
            }),
            Some(api::Opcode::ErrorSubscribe) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let hookdata = buffer.to_original::<api::ScalarHook, _>().unwrap();
                do_hook(hookdata, &mut error_cb_conns);
            }
            Some(api::Opcode::ErrorNotification) => {
                log::error!(
                    "Got a notification interrupt from the TRNG. Syndrome: {:?}",
                    trng.get_errors()
                );
                log::error!("Stats: {:?}", trng.get_err_stats());
                send_event(&error_cb_conns);
            }
            Some(api::Opcode::HealthStats) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                buffer.replace(trng.get_tests()).unwrap();
            }
            Some(api::Opcode::ErrorStats) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                buffer.replace(trng.get_errors()).unwrap();
            }
            Some(api::Opcode::FillTrng) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let len = buffer.as_flat::<TrngBuf, _>().unwrap().len;
                buffer.replace(trng.get_buf(len)).unwrap();
            }
            Some(api::Opcode::TestSetMode) => xous::msg_blocking_scalar_unpack!(msg, mode_code, _, _, _, {
                if let Some(mode) = FromPrimitive::from_usize(mode_code) {
                    test_mode = mode;
                    match mode {
                        TrngTestMode::None => {
                            // de-allocate the test memory
                            test_data.clear();
                            test_data.shrink_to(0);
                        }
                        _ => {
                            // pre-allocate the expected test memory, to avoid thrashing the heap
                            if test_data.capacity() < TEST_CACHE_LEN {
                                test_data.reserve(TEST_CACHE_LEN - test_data.len());
                            }
                        }
                    }
                } else {
                    test_mode = TrngTestMode::None;
                    // de-allocate the test memory
                    test_data.clear();
                    test_data.shrink_to(0);
                }
                xous::return_scalar(msg.sender, 1).ok();
            }),
            Some(api::Opcode::TestGetData) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                buffer.replace(trng.get_trng_testmode(&mut test_data, test_mode)).unwrap();
            }
            Some(api::Opcode::Quit) => break,
            None => {
                log::error!("couldn't convert opcode, ignoring");
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    unhook(&mut error_cb_conns);
    xns.unregister_server(trng_sid).unwrap();
    xous::destroy_server(trng_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn do_hook(hookdata: ScalarHook, cb_conns: &mut [Option<ScalarCallback>; 32]) {
    let (s0, s1, s2, s3) = hookdata.sid;
    let sid = xous::SID::from_u32(s0, s1, s2, s3);
    let server_to_cb_cid = xous::connect(sid).unwrap();
    let cb_dat = Some(ScalarCallback {
        server_to_cb_cid,
        cb_to_client_cid: hookdata.cid,
        cb_to_client_id: hookdata.id,
    });
    let mut found = false;
    for entry in cb_conns.iter_mut() {
        if entry.is_none() {
            *entry = cb_dat;
            found = true;
            break;
        }
    }
    if !found {
        log::error!("ran out of space registering callback");
    }
}
fn unhook(cb_conns: &mut [Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter_mut() {
        if let Some(scb) = entry {
            xous::send_message(
                scb.server_to_cb_cid,
                xous::Message::new_blocking_scalar(
                    api::EventCallback::Drop.to_usize().unwrap(),
                    0,
                    0,
                    0,
                    0,
                ),
            )
            .unwrap();
            unsafe {
                xous::disconnect(scb.server_to_cb_cid).unwrap();
            }
        }
        *entry = None;
    }
}
fn send_event(cb_conns: &[Option<ScalarCallback>; 32]) {
    for entry in cb_conns.iter() {
        if let Some(scb) = entry {
            // note that the "which" argument is only used for GPIO events, to indicate which pin had the event
            xous::send_message(
                scb.server_to_cb_cid,
                xous::Message::new_scalar(
                    api::EventCallback::Event.to_usize().unwrap(),
                    scb.cb_to_client_cid as usize,
                    scb.cb_to_client_id as usize,
                    0,
                    0,
                ),
            )
            .unwrap();
        };
    }
}
