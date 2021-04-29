#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;

use num_traits::FromPrimitive;

use log::info;


#[cfg(target_os = "none")]
mod implementation {
    use utralib::generated::*;
    // use crate::api::*;
    use log::info;
    use susres::{RegManager, RegOrField, SuspendResume};

    pub struct Trng {
        csr: utralib::CSR<u32>,
        // TODO: allocate a software buffer for whitened TRNGs
        susres_manager: RegManager::<{utra::trng_server::TRNG_SERVER_NUMREGS}>, // probably can be reduced to save space?
    }

    impl Trng {
        pub fn new() -> Trng {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::trng_server::HW_TRNG_SERVER_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map TRNG CSR range");

            let mut trng = Trng {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres_manager: RegManager::new(csr.as_mut_ptr() as *mut u32),
            };

            ///// configure power settings and which generator to use
            if !(cfg!(feature = "avalanchetest") || cfg!(feature = "ringosctest")) {
                trng.csr.wo(utra::trng_server::CONTROL,
                    trng.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                   // | self.server_csr.ms(utra::trng_server::CONTROL_AV_DIS, 1)  // disable the AV generator to characterize the RO
                   // | self.server_csr.ms(utra::trng_server::CONTROL_RO_DIS, 1)  // disable the RO to characterize only the AV
                );
                log::trace!("TRNG configured for normal operation (av+ro): 0x{:08x}", trng.csr.r(utra::trng_server::CONTROL));
            } else if cfg!(feature = "avalanchetest") && ! cfg!(feature = "ringosctest") {
                // avalanche test only
                trng.csr.wo(utra::trng_server::CONTROL,
                    trng.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1) // question: do we want power save on or off for testing?? let's leave it on for now, it will make the test run slower but maybe more accurate to our usual use case
                    | trng.csr.ms(utra::trng_server::CONTROL_RO_DIS, 1)  // disable the RO to characterize only the AV
                );
                log::info!("TRNG configured for avalanche testing: 0x{:08x}", trng.csr.r(utra::trng_server::CONTROL));
            } else if ! cfg!(feature = "avalanchetest") && cfg!(feature = "ringosctest") {
                // ring osc test only
                trng.csr.wo(utra::trng_server::CONTROL,
                    trng.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_AV_DIS, 1)  // disable the AV generator to characterize the RO
                );
                log::info!("TRNG configured for ring oscillator testing: 0x{:08x}", trng.csr.r(utra::trng_server::CONTROL));
            } else {
                // both on
                trng.csr.wo(utra::trng_server::CONTROL,
                    trng.csr.ms(utra::trng_server::CONTROL_ENABLE, 1)
                    | trng.csr.ms(utra::trng_server::CONTROL_POWERSAVE, 1)
                );
                log::info!("TRNG configured for both avalanche and ring oscillator testing: 0x{:08x}", trng.csr.r(utra::trng_server::CONTROL));
            }

            trng.susres_manager.push(RegOrField::Reg(utra::trng_server::CONTROL), None);

            /*** TRNG tuning parameters: these were configured and tested in a long run against Dieharder
                 There is a rate of TRNG generation vs. quality trade-off. The tuning below is toward quality of
                 TRNG versus rate of TRNG, such that we could use these without any whitening.
             ***/
            ///// configure avalanche
            // delay in microseconds for avalanche poweron after powersave
            trng.csr.wo(utra::trng_server::AV_CONFIG,
                trng.csr.ms(utra::trng_server::AV_CONFIG_POWERDELAY, 50_000)
                | trng.csr.ms(utra::trng_server::AV_CONFIG_SAMPLES, 32)
            );
            trng.susres_manager.push(RegOrField::Reg(utra::trng_server::AV_CONFIG), None);

            ///// configure ring oscillator
            trng.csr.wo(utra::trng_server::RO_CONFIG,
                trng.csr.ms(utra::trng_server::RO_CONFIG_DELAY, 4)
                | trng.csr.ms(utra::trng_server::RO_CONFIG_DWELL, 100)
                | trng.csr.ms(utra::trng_server::RO_CONFIG_GANG, 1)
                | trng.csr.ms(utra::trng_server::RO_CONFIG_FUZZ, 1)
                | trng.csr.ms(utra::trng_server::RO_CONFIG_OVERSAMPLING, 3)
            );
            trng.susres_manager.push(RegOrField::Reg(utra::trng_server::RO_CONFIG), None);

            info!("hardware initialized");

            trng
        }
        // for the test procedure
        #[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
        pub fn get_trng_csr(&self) -> *mut u32 {
            self.csr.base
        }

        pub fn get_data_eager(&self) -> u32 {
            while self.csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
                xous::yield_slice();
            }
            self.csr.rf(utra::trng_server::DATA_DATA)
        }

        #[allow(dead_code)]
        pub fn wait_full(&self) {
            while self.csr.rf(utra::trng_server::STATUS_FULL) == 0 {
                xous::yield_slice();
            }
        }

        pub fn get_trng(&self, count: usize) -> [u32; 2] {
            // TODO: use SHA hardware unit to robustify the TRNG output against potential hardware failures
            // TODO: health monitoring of raw TRNG output
            let mut ret: [u32; 2] = [0, 0];

            /*
               in the final implementation the algorithm should be:
                 1) check fullness of software-whitened pool
                 2) if software pool is full enough, return values from there
                 3) if pool is low, activate hardware TRNG and refill the pool (uses SHA unit)
                 4) during pool-filling, perform statistics on the hardware TRNG output to check health
                 5) confirm health is OK
            */

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
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use log::info;

    pub struct Trng {
        seed: u32,
    }

    impl Trng {
        pub fn new() -> Trng {
            Trng {
                seed: 0x1afe_cafe,
            }
        }

        fn move_lfsr(&self, mut lfsr: u32) -> u32 {
            lfsr ^= lfsr >> 7;
            lfsr ^= lfsr << 9;
            lfsr ^= lfsr >> 13;
            lfsr
        }

        #[allow(dead_code)]
        pub fn wait_full(&self) { }

        pub fn get_trng(&mut self, _count: usize) -> [u32; 2] {
            info!("hosted mode TRNG is *not* random, it is an LFSR");
            let mut ret: [u32; 2] = [0; 2];
            self.seed = self.move_lfsr(self.seed);
            ret[0] = self.seed;
            self.seed = self.move_lfsr(self.seed);
            ret[1] = self.seed;

            ret
        }
        pub fn suspend(&self) {
        }
        pub fn resume(&self) {
        }
    }
}

#[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
pub const TRNG_BUFF_LEN: usize = 512*1024;
#[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
pub enum WhichMessible {
    One,
    Two,
}
#[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
struct Tester {
    server_csr: utralib::CSR<u32>,
    messible_csr: utralib::CSR<u32>,
    messible2_csr: utralib::CSR<u32>,
    buffer_a: xous::MemoryRange,
    buffer_b: xous::MemoryRange,
    ticktimer: ticktimer_server::Ticktimer,
}
#[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
impl Tester {
    pub fn new(server_csr: *mut u32) -> Tester {
        use utralib::generated::*;
        let buff_a = xous::syscall::map_memory(
            xous::MemoryAddress::new(0x4020_0000), // fix this at a known physical address
            None,
            crate::TRNG_BUFF_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map TRNG comms buffer A");

        let buff_b = xous::syscall::map_memory(
            xous::MemoryAddress::new(0x4030_0000), // fix this at a known physical address
            None,
            crate::TRNG_BUFF_LEN,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map TRNG comms buffer B");

        let messible = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::messible::HW_MESSIBLE_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map messible");

        let messible2 = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::messible2::HW_MESSIBLE2_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map messible");

        Tester {
            server_csr: CSR::new(server_csr),
            buffer_a: buff_a,
            buffer_b: buff_b,
            messible_csr: CSR::new(messible.as_mut_ptr() as *mut u32),
            messible2_csr: CSR::new(messible2.as_mut_ptr() as *mut u32),
            ticktimer: ticktimer_server::Ticktimer::new().unwrap(),
        }
    }

    pub fn messible_send(&mut self, which: WhichMessible, value: u8) {
        use utralib::generated::*;
        match which {
            WhichMessible::One => self.messible_csr.wfo(utra::messible::IN_IN, value as u32),
            WhichMessible::Two => self.messible2_csr.wfo(utra::messible2::IN_IN, value as u32),
        }
    }
    #[allow(dead_code)]
    pub fn messible_get(&mut self, which: WhichMessible) -> u8 {
        use utralib::generated::*;
        match which {
            WhichMessible::One => self.messible_csr.rf(utra::messible::OUT_OUT) as u8,
            WhichMessible::Two => self.messible2_csr.rf(utra::messible2::OUT_OUT) as u8,
        }
    }
    pub fn messible_wait_get(&mut self, which: WhichMessible) -> u8 {
        use utralib::generated::*;
        match which {
            WhichMessible::One => {
                while self.messible_csr.rf(utra::messible::STATUS_HAVE) == 0 {
                    //xous::yield_slice();
                    self.ticktimer.sleep_ms(50).unwrap();
                }
                self.messible_csr.rf(utra::messible::OUT_OUT) as u8
            },
            WhichMessible::Two => {
                while self.messible2_csr.rf(utra::messible2::STATUS_HAVE) == 0 {
                    //xous::yield_slice();
                    self.ticktimer.sleep_ms(50).unwrap();
                }
                self.messible2_csr.rf(utra::messible2::OUT_OUT) as u8
            },
        }
    }

    pub fn get_buff_a(&self) -> *mut u32 {
        self.buffer_a.as_mut_ptr() as *mut u32
    }
    pub fn get_buff_b(&self) -> *mut u32 {
        self.buffer_b.as_mut_ptr() as *mut u32
    }

    pub fn get_data_eager(&self) -> u32 {
        use utralib::generated::*;
        while self.server_csr.rf(utra::trng_server::STATUS_AVAIL) == 0 {
            xous::yield_slice();
        }
        self.server_csr.rf(utra::trng_server::DATA_DATA)
    }
    #[allow(dead_code)]
    pub fn wait_full(&self) {
        use utralib::generated::*;
        while self.server_csr.rf(utra::trng_server::STATUS_FULL) == 0 {
            xous::yield_slice();
        }
    }
}
#[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
fn tester_thread(csr: usize) {
    let mut trng = Tester::new(csr as *mut u32);

    // just create buffers out of pointers. Definitely. Unsafe.
    let buff_a = trng.get_buff_a() as *mut u32;
    let buff_b = trng.get_buff_b() as *mut u32;

    for i in 0..TRNG_BUFF_LEN / 4 {
        // buff_a[i] = trng.get_data_eager();
        unsafe { buff_a.add(i).write_volatile(trng.get_data_eager()) };
    }
    for i in 0..TRNG_BUFF_LEN / 4 {
        // buff_b[i] = trng.get_data_eager();
        unsafe { buff_b.add(i).write_volatile(trng.get_data_eager()) };
    }
    log::info!("TRNG_TESTER: starting service");
    let mut phase = 1;
    trng.messible_send(WhichMessible::One, phase); // indicate buffer A is ready to go

    loop {
        trng.messible_wait_get(WhichMessible::Two);
        phase += 1;
        if phase % 2 == 1 {
            log::info!("TRNG_TESTER: filling A");
            for i in 0..TRNG_BUFF_LEN / 4 {
                //buff_a[i] = trng.get_data_eager();
                unsafe { buff_a.add(i).write_volatile(trng.get_data_eager()) };
            }
            trng.messible_send(WhichMessible::One, phase);
        } else {
            log::info!("TRNG_TESTER: filling B");
            for i in 0..TRNG_BUFF_LEN / 4 {
                //buff_b[i] = trng.get_data_eager();
                unsafe { buff_b.add(i).write_volatile(trng.get_data_eager()) };
            }
            trng.messible_send(WhichMessible::One, phase);
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::Trng;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let trng_sid = xns.register_name(api::SERVER_NAME_TRNG).expect("can't register server");
    log::trace!("registered with NS -- {:?}", trng_sid);

    let mut trng = Trng::new();

    #[cfg(feature = "avalanchetest")]
    log::info!("TRNG built with avalanche test enabled");

    #[cfg(feature = "ringosctest")]
    log::info!("TRNG built with ring oscillator test enabled");

    #[cfg(any(feature = "avalanchetest", feature="ringosctest"))]
    xous::create_thread_1(tester_thread, trng.get_trng_csr() as usize).expect("couldn't create test thread");

    // pump the TRNG hardware to clear the first number out, sometimes it is 0 due to clock-sync issues on the fifo
    trng.get_trng(2);
    log::trace!("ready to accept requests");

    // register a suspend/resume listener
    let mut susres = susres::Susres::new(&xns).expect("couldn't create suspend/resume object");
    let sr_cid = xous::connect(trng_sid).expect("couldn't create suspend callback connection");
    {
        use num_traits::ToPrimitive;
        susres.hook_suspend_callback(api::Opcode::SuspendResume.to_usize().unwrap() as u32, sr_cid).expect("couldn't register suspend/resume listener");
    }

    loop {
        let msg = xous::receive_message(trng_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(api::Opcode::GetTrng) => xous::msg_blocking_scalar_unpack!(msg, count, _, _, _, {
                let val: [u32; 2] = trng.get_trng(count);
                xous::return_scalar2(msg.sender, val[0] as _, val[1] as _)
                    .expect("couldn't return GetTrng request");
            }),
            Some(api::Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                trng.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                trng.resume();
            }),
            None => {
                log::error!("couldn't convert opcode");
                break
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(trng_sid).unwrap();
    xous::destroy_server(trng_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(); loop {}
}
