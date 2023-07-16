#![cfg_attr(not(target_os = "none"), allow(dead_code))]
#![cfg_attr(not(target_os = "none"), allow(unused_imports))]
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::*;

use num_traits::{FromPrimitive, ToPrimitive};
use xous::msg_blocking_scalar_unpack;
use xous_ipc::Buffer;

use log::info;

use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(feature="precursor", feature="renode"))]
mod implementation {
    use crate::api::Sha2Config;
    use utralib::generated::*;
    #[cfg(feature = "event_wait")]
    use crate::api::Opcode;
    #[cfg(feature = "event_wait")]
    use num_traits::*;

    // Note: there is no susres manager for the Sha512 engine, because its state cannot be saved through a full power off
    // instead, we try to delay a suspend until the caller is finished hashing, and if not, we note that and return a failure
    // for the hash result.
    pub(crate) struct Engine512 {
        csr: utralib::CSR<u32>,
        fifo: xous::MemoryRange,
        #[cfg(feature = "event_wait")]
        handler_conn: Option<xous::CID>,
    }

    /*
     Note: in theory, xous::wait_event() should be a more efficient way to do this than yield_slice(),
     as it should resume scheduling immediately upon an interrupt being fired by the sha512 engine, instead
     of just waiting until the next time slice which can be some ms away. However, attempts to use this
     feature seem to indicate there is either a configuration or a hardware problem in triggering the
     interrupts for this block. This is a thing to fix later.
    */
    #[cfg(feature = "event_wait")]
    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let engine512 = unsafe { &mut *(arg as *mut Engine512) };
        // note we are done and clear the pending request, as this is used as a "wait" until interrupt mechanism with xous::wait_event()
        // this message may not be strictly necessary but we added it in an attempt to get the wait_event() to pick up the
        // interrupt; unfortunately, the interrupt completes before wait_event() finishes processing.
        if let Some(conn) = engine512.handler_conn {
            xous::try_send_message(conn,
                xous::Message::new_scalar(Opcode::IrqEvent.to_usize().unwrap(), 0, 0, 0, 0)).map(|_|()).unwrap();
        } else {
            log::error!("|handle_event_irq: COM interrupt, but no connection for notification!")
        }
        engine512.csr.wo(
            utra::sha512::EV_PENDING,
            engine512.csr.r(utra::sha512::EV_PENDING),
        );
    }

    impl Engine512 {
        pub(crate) fn new(_handler_conn: Option<xous::CID>) -> Engine512 {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::sha512::HW_SHA512_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Engine512 CSR range");
            let fifo = xous::syscall::map_memory(
                xous::MemoryAddress::new(utralib::HW_SHA512_MEM),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map Engine512 CSR range");

            #[cfg(not(feature = "event_wait"))]
            let engine512 = Engine512 {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                fifo,
            };

            #[cfg(feature = "event_wait")]
            let engine512 = Engine512 {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                fifo,
                handler_conn: _handler_conn,
            };
            engine512
        }

        pub (crate) fn init(&mut self) {
            #[cfg(feature = "event_wait")]
            xous::claim_interrupt(
                utra::sha512::SHA512_IRQ,
                handle_irq,
                self as *mut Engine512 as *mut usize,
            )
            .expect("couldn't claim irq");
            // note: we can't use a "susres" manager here because this block has un-suspendable state
            // instead, we rely on every usage of this mechanism to explicitly set this enable bit before relying upon it
            #[cfg(feature = "event_wait")]
            self.csr.wfo(utra::sha512::EV_ENABLE_SHA512_DONE, 1);

            // reset the block on boot
            // we don't save this as part of the susres set because
            // on a "proper" resume, the block was already reset by the secure boot process
            // the code path where this really becomes necessary is if we have a WDT reset or
            // some sort of server-restart.
            self.csr.wfo(utra::sha512::CONFIG_RESET, 1); // takes ~32 cycles to complete
        }

        pub(crate) fn setup(&mut self, config: Sha2Config) {
            self.csr.wfo(utra::sha512::POWER_ON, 1);
            match config {
                Sha2Config::Sha512 => {
                    self.csr.wo(
                        utra::sha512::CONFIG,
                        self.csr.ms(utra::sha512::CONFIG_DIGEST_SWAP, 1)
                            | self.csr.ms(utra::sha512::CONFIG_ENDIAN_SWAP, 1)
                            | self.csr.ms(utra::sha512::CONFIG_SHA_EN, 1),
                    );
                }
                Sha2Config::Sha512Trunc256 => {
                    self.csr.wo(
                        utra::sha512::CONFIG,
                        self.csr.ms(utra::sha512::CONFIG_DIGEST_SWAP, 1)
                            | self.csr.ms(utra::sha512::CONFIG_ENDIAN_SWAP, 1)
                            | self.csr.ms(utra::sha512::CONFIG_SHA_EN, 1)
                            | self.csr.ms(utra::sha512::CONFIG_SELECT_256, 1),
                    );
                }
            }
            self.csr.wfo(utra::sha512::COMMAND_HASH_START, 1);
            self.csr.wfo(utra::sha512::EV_ENABLE_SHA512_DONE, 1);
            self.csr.wfo(utra::sha512::POWER_ON, 0);
        }

        pub(crate) fn update(&mut self, buf: &[u8]) {
            self.csr.wfo(utra::sha512::POWER_ON, 1);
            let sha = self.fifo.as_mut_ptr() as *mut u32;
            let sha_byte = self.fifo.as_mut_ptr() as *mut u8;

            // this unsafe version is very slightly faster than the safe version below
            /*
            let src_bfr = buf.as_ptr() as *mut u32;
            for offset in 0 .. buf.len() / 4 {
                unsafe { sha.write_volatile(src_bfr.add(offset).read_volatile()); }
            }
            if (buf.len() % 4) != 0 {
                for index in (buf.len() - (buf.len() % 4))..buf.len() {
                    unsafe{ sha_byte.write_volatile(buf[index]); }
                }
            } */
            for (_reg, chunk) in buf.chunks(4).enumerate() {
                let mut temp: [u8; 4] = Default::default();
                if chunk.len() == 4 {
                    temp.copy_from_slice(chunk);
                    let dword: u32 = u32::from_le_bytes(temp);

                    while self.csr.rf(utra::sha512::FIFO_ALMOST_FULL) != 0 {
                        xous::yield_slice();
                    }
                    unsafe {
                        sha.write_volatile(dword);
                    }
                } else {
                    for index in 0..chunk.len() {
                        while self.csr.rf(utra::sha512::FIFO_ALMOST_FULL) != 0 {
                            xous::yield_slice();
                        }
                        unsafe {
                            sha_byte.write_volatile(chunk[index]);
                        }
                    }
                }
            }
            self.csr.wfo(utra::sha512::POWER_ON, 0);
        }

        pub(crate) fn finalize(&mut self) -> ([u8; 64], u64) {
            self.csr.wfo(utra::sha512::POWER_ON, 1);
            #[cfg(feature = "event_wait")]
            {
                self.csr.wo(
                    utra::sha512::EV_PENDING,
                    self.csr.r(utra::sha512::EV_PENDING),
                );
                self.csr.wfo(utra::sha512::EV_ENABLE_SHA512_DONE, 1);
                log::trace!("starting sha512_done");
                self.csr.wfo(utra::sha512::COMMAND_HASH_PROCESS, 1);
                while self.csr.rf(utra::sha512::FIFO_RUNNING) == 1 {
                    // race condition between when this kicks off, and when the IRQ event comes in; in the end,
                    // the overhead of wait_event() is longer than it takes for the computation to finish,
                    // so the interrupt arrives *before* this call happens.
                    xous::wait_event();
                }
                log::info!("moving on");
            }
            #[cfg(not(feature = "event_wait"))]
            {
                self.csr.wfo(utra::sha512::COMMAND_HASH_PROCESS, 1);
                while self.csr.rf(utra::sha512::EV_PENDING_SHA512_DONE) == 0 {
                    // don't even call the OS idle, the computation is 2us and the syscall takes longer than that.
                }
                self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1);
            }
            let length_in_bits: u64 = (self.csr.r(utra::sha512::MSG_LENGTH0) as u64)
                | ((self.csr.r(utra::sha512::MSG_LENGTH1) as u64) << 32);
            let mut hash: [u8; 64] = [0; 64];
            let digest_regs: [utralib::Register; 16] = [
                utra::sha512::DIGEST00,
                utra::sha512::DIGEST01,
                utra::sha512::DIGEST10,
                utra::sha512::DIGEST11,
                utra::sha512::DIGEST20,
                utra::sha512::DIGEST21,
                utra::sha512::DIGEST30,
                utra::sha512::DIGEST31,
                utra::sha512::DIGEST40,
                utra::sha512::DIGEST41,
                utra::sha512::DIGEST50,
                utra::sha512::DIGEST51,
                utra::sha512::DIGEST60,
                utra::sha512::DIGEST61,
                utra::sha512::DIGEST70,
                utra::sha512::DIGEST71,
            ];
            let mut i = 0;
            for &reg in digest_regs.iter() {
                hash[i..i + 4].clone_from_slice(&self.csr.r(reg).to_le_bytes());
                i += 4;
            }
            self.csr.wo(utra::sha512::CONFIG, 0); // clear all config bits, including EN, which resets the unit

            self.csr.wfo(utra::sha512::POWER_ON, 0);
            (hash, length_in_bits)
        }

        pub(crate) fn reset(&mut self) {
            self.csr.wfo(utra::sha512::POWER_ON, 1);
            self.csr.wfo(utra::sha512::CONFIG_RESET, 1);
            self.csr.wfo(utra::sha512::EV_PENDING_SHA512_DONE, 1);
            self.csr.wo(utra::sha512::CONFIG, 0); // clear all config bits, including EN, which resets the unit
            while self.csr.rf(utra::sha512::FIFO_RESET_STATUS) == 1 {} // wait for the reset block to finish, if it's not already done by now
            self.csr.wfo(utra::sha512::POWER_ON, 0);
        }

        pub(crate) fn is_idle(&mut self) -> bool {
            self.csr.wfo(utra::sha512::POWER_ON, 1);
            if self.csr.rf(utra::sha512::CONFIG_SHA_EN) == 0
                && self.csr.rf(utra::sha512::FIFO_RUNNING) == 0
            {
                self.csr.wfo(utra::sha512::POWER_ON, 0);
                true
            } else {
                self.csr.wfo(utra::sha512::POWER_ON, 0);
                false
            }
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use crate::Sha2Config;
    use log::info;

    pub(crate) struct Engine512 {}

    impl Engine512 {
        pub(crate) fn new(_handler_conn: Option<xous::CID>) -> Engine512 {
            Engine512 {}
        }
        pub(crate) fn init(&mut self) {}
        pub(crate) fn suspend(&self) {}
        pub(crate) fn resume(&self) {}
        pub(crate) fn reset(&self) {}
        pub(crate) fn setup(&mut self, _config: Sha2Config) {}
        pub(crate) fn update(&mut self, _buf: &[u8]) {}
        pub(crate) fn finalize(&mut self) -> ([u8; 64], u64) {
            ([0; 64], 0)
        }
        pub(crate) fn is_idle(&mut self) -> bool {
            false
        }
    }
}

static HASH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static SUSPEND_FAILURE: AtomicBool = AtomicBool::new(false);
static SUSPEND_PENDING: AtomicBool = AtomicBool::new(false);

fn susres_thread(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let susres_sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    let xns = xous_names::XousNames::new().unwrap();

    // register a suspend/resume listener
    let sr_cid = xous::connect(susres_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, api::SusResOps::SuspendResume as u32, sr_cid)
        .expect("couldn't create suspend/resume object");

    log::trace!("starting Sha512 suspend/resume manager loop");
    loop {
        let msg = xous::receive_message(susres_sid).unwrap();
        log::trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(SusResOps::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                SUSPEND_PENDING.store(true, Ordering::Relaxed);
                while HASH_IN_PROGRESS.load(Ordering::Relaxed) {
                    xous::yield_slice();
                }
                if susres
                    .suspend_until_resume(token)
                    .expect("couldn't execute suspend/resume")
                    == false
                {
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

fn main() -> ! {
    use crate::implementation::Engine512;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // anyone is allowed to connect to this service; authentication by tokens used
    let engine512_sid = xns
        .register_name(api::SERVER_NAME_SHA512, None)
        .expect("can't register server");
    log::trace!("registered with NS -- {:?}", engine512_sid);

    #[cfg(feature="event_wait")]
    let mut engine512 = Box::new(Engine512::new(xous::connect(engine512_sid).ok()));
    #[cfg(not(feature="event_wait"))]
    let mut engine512 = Box::new(Engine512::new(None));
    engine512.init();

    log::trace!("ready to accept requests");

    // handle suspend/resume with a separate thread, which monitors our in-progress state
    // we can't save hardware state of a hash, so the hash MUST finish before we can suspend.
    let susres_mgr_sid = xous::create_server().unwrap();
    let (sid0, sid1, sid2, sid3) = susres_mgr_sid.to_u32();
    xous::create_thread_4(
        susres_thread,
        sid0 as usize,
        sid1 as usize,
        sid2 as usize,
        sid3 as usize,
    )
    .expect("couldn't start susres handler thread");

    let mut client_id: Option<[u32; 3]> = None;
    let mut mode: Option<Sha2Config> = None;
    let mut job_count = 0;
    loop {
        let mut msg = xous::receive_message(engine512_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::AcquireExclusive) => {
                msg_blocking_scalar_unpack!(msg, id0, id1, id2, flags, {
                    if client_id.is_none() && !SUSPEND_PENDING.load(Ordering::Relaxed) {
                        client_id = Some([id0 as u32, id1 as u32, id2 as u32]);
                        //log::trace!("giving {:x?} an exclusive lock", client_id);
                        mode = Some(FromPrimitive::from_usize(flags).unwrap());
                        SUSPEND_FAILURE.store(false, Ordering::Relaxed);
                        HASH_IN_PROGRESS.store(true, Ordering::Relaxed);
                        engine512.setup(mode.unwrap());
                        xous::return_scalar(msg.sender, 1).unwrap();
                    } else {
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }
                })
            }
            Some(Opcode::Reset) => msg_blocking_scalar_unpack!(msg, r_id0, r_id1, r_id2, _, {
                match client_id {
                    Some([id0, id1, id2]) => {
                        if id0 == r_id0 as u32 && id1 == r_id1 as u32 && id2 == r_id2 as u32 {
                            SUSPEND_FAILURE.store(false, Ordering::Relaxed);
                            HASH_IN_PROGRESS.store(false, Ordering::Relaxed);
                            client_id = None;
                            mode = None;
                            engine512.reset();
                            xous::return_scalar(msg.sender, 1).unwrap();
                        } else {
                            xous::return_scalar(msg.sender, 0).unwrap();
                        }
                    }
                    _ => {
                        xous::return_scalar(msg.sender, 0).unwrap();
                    }
                }
            }),
            Some(Opcode::Update) => {
                let buffer =
                    unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let update = buffer.as_flat::<Sha2Update, _>().unwrap();
                match client_id {
                    Some(id) => {
                        if id == update.id {
                            engine512.update(&update.buffer[..update.len as usize]);
                        }
                    }
                    _ => {
                        log::error!("Received a SHA-2 block, but the client ID did not match! Ignoring block.");
                    }
                }
            }
            Some(Opcode::Finalize) => {
                if job_count % 100 == 0 {
                    log::info!("sha512 job {}", job_count); // leave this here for now so we can confirm HW accel is being used when we think it is!
                }
                job_count += 1;
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                let mut finalized = buffer.to_original::<Sha2Finalize, _>().unwrap();
                match client_id {
                    Some(id) => {
                        if id == finalized.id {
                            if SUSPEND_FAILURE.load(Ordering::Relaxed) {
                                finalized.result = Sha2Result::SuspendError;
                                finalized.length_in_bits = None;
                            } else {
                                let (hash, length_in_bits) = engine512.finalize();
                                match mode {
                                    Some(Sha2Config::Sha512) => {
                                        finalized.result = Sha2Result::Sha512Result(hash);
                                        finalized.length_in_bits = Some(length_in_bits);
                                    }
                                    Some(Sha2Config::Sha512Trunc256) => {
                                        let mut trunc: [u8; 32] = [0; 32];
                                        trunc.clone_from_slice(&hash[..32]);
                                        finalized.result = Sha2Result::Sha512Trunc256Result(trunc);
                                        finalized.length_in_bits = Some(length_in_bits);
                                    }
                                    None => {
                                        finalized.result = Sha2Result::Uninitialized;
                                    }
                                }
                            }
                        } else {
                            finalized.result = Sha2Result::IdMismatch;
                            finalized.length_in_bits = None;
                        }
                    }
                    _ => {
                        log::error!(
                            "Received a SHA-2 finalize call, but we aren't doing a hash. Ignoring."
                        );
                    }
                }
                buffer
                    .replace(finalized)
                    .expect("couldn't return hash result");
            }
            Some(Opcode::IsIdle) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if engine512.is_idle() {
                    xous::return_scalar(msg.sender, 1).expect("couldn't return IsIdle query");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't return IsIdle query");
                }
            }),
            Some(Opcode::AcquireSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if client_id.is_none() {
                    SUSPEND_PENDING.store(true, Ordering::Relaxed);
                    xous::return_scalar(msg.sender, 1).expect("couldn't ack AcquireSuspendLock");
                } else {
                    xous::return_scalar(msg.sender, 0).expect("couldn't ack AcquireSuspendLock");
                }
            }),
            Some(Opcode::AbortSuspendLock) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                SUSPEND_PENDING.store(false, Ordering::Relaxed);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack AbortSuspendLock");
            }),
            #[cfg(feature = "event_wait")]
            Some(Opcode::IrqEvent) => {
                log::info!("irq_event");
                // nothing to do; the purpose is just to wake up the thread as it sleeps
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
    let quitconn = xous::connect(susres_mgr_sid).unwrap();
    xous::send_message(
        quitconn,
        xous::Message::new_scalar(SusResOps::Quit.to_usize().unwrap(), 0, 0, 0, 0),
    )
    .unwrap();
    unsafe {
        xous::disconnect(quitconn).unwrap();
    }

    xns.unregister_server(engine512_sid).unwrap();
    xous::destroy_server(engine512_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
