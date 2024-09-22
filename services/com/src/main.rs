#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use core::convert::TryInto;

use api::*;
use com_rs::serdes::{Ipv4Conf, StringSer, STR_32_WORDS, STR_64_WORDS};
use com_rs::*;
use log::{error, info, trace};
use num_traits::{FromPrimitive, ToPrimitive};
use xous::{msg_blocking_scalar_unpack, msg_scalar_unpack, CID};
use xous_ipc::Buffer;

const LEGACY_REV: u32 = 0x8b5b_8e50; // this is the git rev shipped before we went to version tagging
const LEGACY_TAG: u32 = 0x00_09_05_00; // this is corresponding tag
const STD_TIMEOUT: u32 = 100;
const EC_BOOT_WAIT_MS: usize = 3500;
#[derive(Debug, Copy, Clone)]
pub struct WorkRequest {
    work: ComSpec,
    sender: CID,
}

fn return_battstats(cid: CID, stats: api::BattStats) -> Result<(), xous::Error> {
    let rawstats: [usize; 2] = stats.into();
    xous::send_message(
        cid,
        xous::Message::new_scalar(
            api::Callback::BattStats.to_usize().unwrap(),
            rawstats[0],
            rawstats[1],
            0,
            0,
        ),
    )
    .map(|_| ())
}

#[cfg(any(feature = "precursor", feature = "renode"))]
mod implementation {
    use com_rs::*;
    use log::error;
    use susres::{RegManager, RegOrField, SuspendResume};
    use utralib::generated::*;

    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;

    const STD_TIMEOUT: u32 = 100;

    pub struct XousCom {
        csr: utralib::CSR<u32>,
        susres: RegManager<{ utra::com::COM_NUMREGS }>,
        ticktimer: ticktimer_server::Ticktimer,
        pub workqueue: Vec<WorkRequest>,
        busy: bool,
        stby_current: Option<i16>,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let xc = unsafe { &mut *(arg as *mut XousCom) };
        // just clear the pending request, as this is used as a "wait" until request function
        xc.csr.wo(utra::com::EV_PENDING, xc.csr.r(utra::com::EV_PENDING));
    }

    impl XousCom {
        pub fn new() -> XousCom {
            let csr = xous::syscall::map_memory(
                xous::MemoryAddress::new(utra::com::HW_COM_BASE),
                None,
                4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map COM CSR range");

            let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");

            XousCom {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                ticktimer,
                workqueue: Vec::new(),
                busy: false,
                stby_current: None,
            }
        }

        pub fn init(&mut self) {
            xous::claim_interrupt(utra::com::COM_IRQ, handle_irq, self as *mut XousCom as *mut usize)
                .expect("couldn't claim irq");

            self.susres.push(RegOrField::Reg(utra::com::CONTROL), None);
            self.susres.push_fixed_value(RegOrField::Reg(utra::com::EV_PENDING), 0xFFFF_FFFF);
            self.susres.push(RegOrField::Reg(utra::com::EV_ENABLE), None);
        }

        pub fn suspend(&mut self) {
            self.susres.suspend();
            self.csr.wo(utra::com::EV_ENABLE, 0);
            self.csr.wo(utra::com::EV_PENDING, 0xFFFF_FFFF);
        }

        pub fn resume(&mut self) {
            self.susres.resume();
            // issue a "link sync" command because the COM had continued running, and we may have sent garbage
            // during suspend
            self.txrx(ComState::LINK_SYNC.verb);
            // wait a moment for the link to stabilize, before allowing any other commands to issue
            self.ticktimer.sleep_ms(5).unwrap();
        }

        pub fn txrx(&mut self, tx: u16) -> u16 {
            self.csr.wfo(utra::com::TX_TX, tx as u32);
            /* transaction is automatically initiated on write
            wait while transaction is in progress. A transaction takes about 80-100 CPU cycles;
            not quite enough to be worth the overhead of an interrupt, so we just yield our time slice */
            while self.csr.rf(utra::com::STATUS_TIP) == 1 {
                // xous::yield_slice();
                /* ... and it turns out yielding the slice is a bad idea, because you may not get re-scheduled
                for a very long time, which causes the COM responder to timeout. Just waste the cycles. */
            }

            // grab the RX value and return it
            self.csr.rf(utra::com::RX_RX) as u16
        }

        pub fn wait_txrx(&mut self, tx: u16, timeout: Option<u32>) -> u16 {
            if timeout.is_some() {
                let curtime = self.ticktimer.elapsed_ms();
                let mut timed_out = false;
                let to = timeout.unwrap() as u64;
                while self.csr.rf(utra::com::STATUS_HOLD) == 1 && !timed_out {
                    if (self.ticktimer.elapsed_ms() - curtime) > to {
                        log::warn!("COM timeout");
                        timed_out = true;
                    }
                    xous::yield_slice();
                }
            } else {
                while self.csr.rf(utra::com::STATUS_HOLD) == 1 {
                    self.csr.wfo(utra::com::EV_ENABLE_SPI_HOLD, 1);
                    xous::wait_event();
                    self.csr.wfo(utra::com::EV_ENABLE_SPI_HOLD, 0);
                }
            }

            self.txrx(tx)
        }

        pub fn try_wait_txrx(&mut self, tx: u16, timeout: u32) -> Option<u16> {
            let curtime = self.ticktimer.elapsed_ms();
            let mut timed_out = false;
            let to = timeout as u64;
            while self.csr.rf(utra::com::STATUS_HOLD) == 1 && !timed_out {
                if (self.ticktimer.elapsed_ms() - curtime) > to {
                    log::warn!("COM timeout");
                    timed_out = true;
                }
                xous::yield_slice();
            }
            if timed_out {
                self.txrx(tx); // still push the packet in, so that the interface is pumped
                None // but note the failure
            } else {
                Some(self.txrx(tx))
            }
        }

        pub fn process_queue(&mut self) -> Option<xous::CID> {
            if !self.workqueue.is_empty() && !self.busy {
                self.busy = true;
                let work_descriptor = self.workqueue.remove(0);
                let ret = if work_descriptor.work.verb == ComState::STAT.verb {
                    let stats = self.get_battstats();
                    match return_battstats(work_descriptor.sender, stats) {
                        Err(xous::Error::ServerNotFound) => {
                            // the callback target has quit, so de-allocate it from our list
                            Some(work_descriptor.sender)
                        }
                        Ok(()) => None,
                        _ => panic!("unhandled error in callback process_queue"),
                    }
                } else {
                    error!("unimplemented work queue responder 0x{:x}", work_descriptor.work.verb);
                    None
                };
                self.busy = false;
                ret
            } else {
                None
            }
        }

        pub fn stby_current(&self) -> Option<i16> { self.stby_current }

        pub fn get_battstats(&mut self) -> BattStats {
            let mut stats = BattStats::default();

            self.txrx(ComState::GAS_GAUGE.verb);
            stats.current = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as i16;
            self.stby_current = Some(self.wait_txrx(ComState::LINK_READ.verb, Some(100)) as i16);
            stats.voltage = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            self.wait_txrx(ComState::LINK_READ.verb, Some(100)); // power register value, not used

            self.txrx(ComState::GG_SOC.verb);
            stats.soc = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
            self.txrx(ComState::GG_REMAINING.verb);
            stats.remaining_capacity = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));

            stats
        }

        pub fn get_more_stats(&mut self) -> [u16; 15] {
            self.txrx(ComState::STAT.verb);
            let ack = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            if ack != 0x8888 {
                log::error!("didn't receive the expected ack header to the stats readout");
            }
            let mut ret: [u16; 15] = [0; 15];
            for r in ret.iter_mut() {
                *r = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            }
            ret
        }

        pub fn poll_usb_cc(&mut self) -> [u32; 2] {
            self.txrx(ComState::POLL_USB_CC.verb);
            let event = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            let mut ret: [u16; 3] = [0; 3];
            for r in ret.iter_mut() {
                *r = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            }
            let rev = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            // pack into a format that can be returned as a scalar2
            [
                ((rev & 0xff) as u32) << 24 | (((event & 0xff) as u32) << 16) | ret[0] as u32,
                ret[1] as u32 | ((ret[2] as u32) << 16),
            ]
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "xous"))]
mod implementation {
    use com_rs::*;
    use log::error;

    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;

    pub struct XousCom {
        pub workqueue: Vec<WorkRequest>,
        busy: bool,
    }

    impl XousCom {
        pub fn new() -> XousCom { XousCom { workqueue: Vec::new(), busy: false } }

        pub fn init(&mut self) {}

        pub fn suspend(&self) {}

        pub fn resume(&self) {}

        pub fn txrx(&mut self, _tx: u16) -> u16 { 0xDEAD as u16 }

        pub fn wait_txrx(&mut self, _tx: u16, _timeout: Option<u32>) -> u16 { 0xDEAD as u16 }

        pub fn try_wait_txrx(&mut self, _tx: u16, _timeout: u32) -> Option<u16> { None }

        pub fn get_battstats(&mut self) -> BattStats {
            BattStats { voltage: 3950, current: -110, soc: 85, remaining_capacity: 850 }
        }

        pub fn stby_current(&self) -> Option<i16> { None }

        pub fn process_queue(&mut self) -> Option<xous::CID> {
            if !self.workqueue.is_empty() && !self.busy {
                self.busy = true;
                let work_descriptor = self.workqueue.remove(0);
                let ret = if work_descriptor.work.verb == ComState::STAT.verb {
                    let stats = self.get_battstats();
                    match return_battstats(work_descriptor.sender, stats) {
                        Err(xous::Error::ServerNotFound) => {
                            // the callback target has quit, so de-allocate it from our list
                            Some(work_descriptor.sender)
                        }
                        Ok(()) => None,
                        _ => panic!("unhandled error in callback process_queue"),
                    }
                } else {
                    error!("unimplemented work queue responder 0x{:x}", work_descriptor.work.verb);
                    None
                };
                self.busy = false;
                ret
            } else {
                None
            }
        }

        pub fn get_more_stats(&mut self) -> [u16; 15] {
            let mut ret = [0u16; 15];
            ret[12] = 3950; // make the oqc test happy
            ret
        }

        pub fn poll_usb_cc(&mut self) -> [u32; 2] { [0; 2] }
    }
}

fn main() -> ! {
    use crate::implementation::XousCom;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // reset the EC so we're in sync at boot on state
    let llio = llio::Llio::new(&xns);
    llio.ec_reset().unwrap();

    // unlimited connections allowed -- any server is currently allowed to talk to COM. This might need to be
    // revisited.
    let com_sid = xns.register_name(api::SERVER_NAME_COM, None).expect("can't register server");
    trace!("registered with NS -- {:?}", com_sid);

    // Create a new com object
    let mut com = Box::new(XousCom::new());
    com.init();
    let ticktimer = ticktimer_server::Ticktimer::new().unwrap();

    #[cfg(not(any(windows, unix)))] // avoid errors in hosted mode
    {
        ticktimer.sleep_ms(100).unwrap(); // give the EC a moment to de-chatter
        let mut attempts = 0;
        let ping_value = 0xaeedu16; // for various reasons, this is a string that is "unlikely" to happen randomly
        loop {
            com.txrx(ComState::LINK_PING.verb);
            com.txrx(ping_value);
            let pong = com.wait_txrx(ComState::LINK_READ.verb, Some(150)); // this should "stall" until the EC comes out of reset
            let phase = com.wait_txrx(ComState::LINK_READ.verb, Some(150));
            if pong == !ping_value && phase == 0x600d {
                // 0x600d is a hard-coded constant. It's included to confirm that we aren't "wedged" just
                // sending one value back at us
                log::info!("EC rebooting: link established");
                break;
            } else {
                log::info!(
                    "EC rebooting: establishing link sync, attempt {} [{:04x}/{:04x}]",
                    attempts,
                    pong,
                    phase
                );
                com.txrx(ComState::LINK_SYNC.verb);
                ticktimer.sleep_ms(200).unwrap();
                attempts += 1;
            }
            if attempts > 50 {
                log::error!("EC didn't sync out of reset...continuing and praying for the best.");
                break;
            }
        }
    }

    // register a suspend/resume listener
    let sr_cid = xous::connect(com_sid).expect("couldn't create suspend callback connection");
    let mut susres =
        susres::Susres::new(Some(susres::SuspendOrder::Late), &xns, Opcode::SuspendResume as u32, sr_cid)
            .expect("couldn't create suspend/resume object");

    // create an array to track return connections for battery stats TODO: refactor this to use a Vec instead
    // of static allocations
    let mut battstats_conns: [Option<xous::CID>; 32] = [None; 32];
    // other future notification vectors shall go here

    let mut bl_main = 0;
    let mut bl_sec = 0;

    let mut flash_id: Option<[u32; 4]> = None; // only one process can acquire this, and its ID is stored here.
    const FLASH_LEN: u32 = 0x10_0000;
    const FLASH_TIMEOUT: u32 = 250;

    // initial seed of the COM trng
    ticktimer.sleep_ms(100).unwrap(); // some time for the EC to catch up during boot
    let trng = trng::Trng::new(&xns).expect("couldn't connect to TRNG");
    com.txrx(ComState::TRNG_SEED.verb);
    for _ in 0..2 {
        let mut rng = trng.get_u64().expect("couldn't fetch rngs");
        for _ in 0..4 {
            com.txrx(rng as u16);
            rng >>= 16;
        }
    }

    // determine the version of the COM that we're talking to
    ticktimer.sleep_ms(100).unwrap(); // some time for the EC to catch up during boot
    let mut ec_git_rev = {
        com.txrx(ComState::EC_GIT_REV.verb);
        let rev_msb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT * 2)) as u16;
        let rev_lsb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT * 2)) as u16;
        let _dirty = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT * 2)) as u8;
        ((rev_msb as u32) << 16) | (rev_lsb as u32)
    };
    let mut ec_tag = { if ec_git_rev == LEGACY_REV { LEGACY_TAG } else { parse_version(&mut com) } };
    let mut desired_int_mask = 0;

    trace!("starting main loop");
    loop {
        let mut msg = xous::receive_message(com_sid).unwrap();
        let opcode: Option<Opcode> = FromPrimitive::from_usize(msg.body.id());
        log::debug!("{:?}", opcode);
        match opcode {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                com.txrx(ComState::LINK_SET_INTMASK.verb);
                com.txrx(0); // suppress interrupts on suspend

                if bl_main != 0 || bl_sec != 0 {
                    com.txrx(ComState::BL_START.verb); // this will turn off the backlights
                }
                com.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                com.resume();

                com.txrx(ComState::LINK_SET_INTMASK.verb);
                com.txrx(desired_int_mask); // restore interrupts on resume

                if bl_main != 0 || bl_sec != 0 {
                    // restore the backlight settings, if they are not 0
                    com.txrx(
                        ComState::BL_START.verb | (bl_main as u16) & 0x1f | (((bl_sec as u16) & 0x1f) << 5),
                    );
                }
            }),
            Some(Opcode::Ping) => xous::msg_blocking_scalar_unpack!(msg, ping, _, _, _, {
                xous::return_scalar(msg.sender, !ping).unwrap();
            }),
            Some(Opcode::LinkReset) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::LINK_SYNC.verb);
                ticktimer.sleep_ms(200).unwrap(); // give some time for the link to reset - the EC does not have a guaranteed latency to respond to a link reset
                if ec_tag >= u32::from_be_bytes(ComState::LINK_PING.apilevel) {
                    let mut attempts = 0;
                    let ping_value = 0xaeedu16; // for various reasons, this is a string that is "unlikely" to happen randomly
                    loop {
                        com.txrx(ComState::LINK_PING.verb);
                        com.txrx(ping_value);
                        let pong = com.wait_txrx(ComState::LINK_READ.verb, Some(500));
                        let phase = com.wait_txrx(ComState::LINK_READ.verb, Some(500));
                        if pong == !ping_value && phase == 0x600d {
                            // 0x600d is a hard-coded constant. It's included to confirm that we aren't
                            // "wedged" just sending one value back at us
                            break;
                        } else {
                            log::warn!(
                                "Link reset: establishing link sync, attempt {} [{:04x}/{:04x}]",
                                attempts,
                                pong,
                                phase
                            );
                            com.txrx(ComState::LINK_SYNC.verb);
                            ticktimer.sleep_ms(200).unwrap();
                            attempts += 1;
                        }
                        if attempts > 50 {
                            // abort and return failure
                            xous::return_scalar(msg.sender, 0).unwrap();
                            continue;
                        }
                    }
                } else {
                    // replace with a dead wait loop for older revs
                    log::warn!("EC rev is too old: replacing link SYNC with a dead wait loop");
                    ticktimer.sleep_ms(5000).unwrap();
                }
                xous::return_scalar(msg.sender, 1).unwrap();
            }),
            Some(Opcode::ReseedTrng) => xous::msg_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::TRNG_SEED.verb);
                for _ in 0..2 {
                    let mut rng = trng.get_u64().expect("couldn't fetch rngs");
                    for _ in 0..4 {
                        com.txrx(rng as u16);
                        rng >>= 16;
                    }
                }
            }),
            Some(Opcode::GetUptime) => xous::msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let mut uptime: u64 = 0;
                com.txrx(ComState::UPTIME.verb);
                for _ in 0..4 {
                    uptime >>= 16;
                    uptime |= (com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u64) << 48;
                }
                xous::return_scalar2(msg.sender, uptime as usize, (uptime >> 32) as usize)
                    .expect("couldn't return uptime");
            }),
            Some(Opcode::FlashAcquire) => msg_blocking_scalar_unpack!(msg, id0, id1, id2, id3, {
                let acquired = if flash_id.is_none() {
                    flash_id = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                    1
                } else {
                    0
                };
                xous::return_scalar(msg.sender, acquired as usize)
                    .expect("couldn't acknowledge acquire message");
            }),
            Some(Opcode::FlashOp) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut flash_op = buffer.to_original::<api::FlashRecord, _>().unwrap();
                let mut pass = false;
                if let Some(id) = flash_id {
                    if id == flash_op.id {
                        match &mut flash_op.op {
                            api::FlashOp::Erase(addr, len) => {
                                if *addr < FLASH_LEN && *len + *addr < FLASH_LEN {
                                    log::debug!(
                                        "Erasing EC region starting at 0x{:x}, lenth 0x{:x}",
                                        *addr,
                                        *len
                                    );
                                    com.txrx(ComState::FLASH_ERASE.verb);
                                    com.txrx((*addr >> 16) as u16);
                                    com.txrx(*addr as u16);
                                    com.txrx((*len >> 16) as u16);
                                    com.txrx(*len as u16);
                                    while ComState::FLASH_ACK.verb
                                        != com.wait_txrx(ComState::FLASH_WAITACK.verb, Some(FLASH_TIMEOUT))
                                    {
                                        xous::yield_slice();
                                    }
                                    pass = true;
                                } else {
                                    pass = false;
                                }
                            }
                            api::FlashOp::Program(addr, some_pages) => {
                                com.txrx(ComState::FLASH_LOCK.verb);
                                let mut prog_ptr = *addr;
                                // this will fill the 1280-deep FIFO with up to 4 pages of data for
                                // programming
                                pass = true;
                                for &maybe_page in some_pages.iter() {
                                    if prog_ptr < FLASH_LEN - 256 {
                                        log::trace!("Prog EC page at 0x{:x}", prog_ptr);
                                        if let Some(page) = maybe_page {
                                            com.txrx(ComState::FLASH_PP.verb);
                                            com.txrx((prog_ptr >> 16) as u16);
                                            com.txrx(prog_ptr as u16);
                                            for i in 0..128 {
                                                com.txrx(
                                                    page[i * 2] as u16 | ((page[i * 2 + 1] as u16) << 8),
                                                );
                                            }
                                        }
                                        prog_ptr += 256;
                                    } else {
                                        pass = false;
                                    }
                                }
                                // wait for completion only after all 4 pages are sent
                                while ComState::FLASH_ACK.verb
                                    != com.wait_txrx(ComState::FLASH_WAITACK.verb, Some(FLASH_TIMEOUT))
                                {
                                    xous::yield_slice();
                                }
                                com.txrx(ComState::FLASH_UNLOCK.verb);
                            }
                            api::FlashOp::Verify(addr, data) => {
                                if ec_tag >= u32::from_be_bytes(ComState::FLASH_VERIFY.apilevel) {
                                    com.txrx(ComState::FLASH_VERIFY.verb);
                                    com.txrx((*addr >> 16) as u16);
                                    com.txrx(*addr as u16);

                                    for word in data.chunks_mut(2) {
                                        let read = com
                                            .wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT))
                                            .to_le_bytes();
                                        word[0] = read[0];
                                        word[1] = read[1];
                                    }
                                    pass = true;
                                } else {
                                    for d in data.iter_mut() {
                                        *d = 0;
                                    }
                                    pass = false;
                                }
                            }
                        }
                    }
                } else {
                    pass = false;
                }
                match flash_op.op {
                    api::FlashOp::Verify(_a, data) => {
                        log::debug!("verify returning {:x?}", &data[..32]);
                        buffer.replace(flash_op).expect("couldn't return result on FlashOp");
                    }
                    _ => {
                        let response = if pass { api::FlashResult::Pass } else { api::FlashResult::Fail };
                        buffer.replace(response).expect("couldn't return result on FlashOp");
                    }
                }
            }
            Some(Opcode::RegisterBattStatsListener) => msg_scalar_unpack!(msg, sid0, sid1, sid2, sid3, {
                let sid = xous::SID::from_u32(sid0 as _, sid1 as _, sid2 as _, sid3 as _);
                let cid = Some(xous::connect(sid).unwrap());
                let mut found = false;
                for entry in battstats_conns.iter_mut() {
                    if *entry == None {
                        *entry = cid;
                        found = true;
                        break;
                    }
                }
                if !found {
                    error!("RegisterBattStatsListener ran out of space registering callback");
                }
            }),
            Some(Opcode::IsCharging) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::POWER_CHARGER_STATE.verb);
                let result = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                xous::return_scalar(msg.sender, result as usize).expect("couldn't return charging state");
            }),
            Some(Opcode::RequestCharging) => msg_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::CHG_START.verb);
            }),
            Some(Opcode::StandbyCurrent) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                if let Some(i) = com.stby_current() {
                    xous::return_scalar2(msg.sender, 1, i as usize).expect("couldn't return StandbyCurrent");
                } else {
                    xous::return_scalar2(msg.sender, 0, 0).expect("couldn't return StandbyCurrent");
                }
            }),
            Some(Opcode::Wf200PdsLine) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let l = buffer.to_original::<String, _>().unwrap();
                info!("Wf200PdsLine got line {}", l);
                let line = l.as_bytes();
                let length = (l.len() + 0) as u16;
                //info!("0x{:04x}", ComState::WFX_PDS_LINE_SET.verb);
                com.txrx(ComState::WFX_PDS_LINE_SET.verb);
                //info!("0x{:04x}", length);
                com.txrx(length);
                //for i in 0..(ComState::WFX_PDS_LINE_SET.w_words as usize - 1) {
                for i in 0..128 {
                    let word: u16;
                    if (i * 2 + 1) == length as usize {
                        // odd last element
                        word = line[i * 2] as u16;
                    } else if i * 2 < length as usize {
                        word = (line[i * 2] as u16) | ((line[i * 2 + 1] as u16) << 8);
                    } else {
                        word = 0;
                    }
                    com.txrx(word);
                    //info!("0x{:04x}", word);
                }
            }
            Some(Opcode::PowerOffSoc) => {
                // NOTE: this is deprecated, use susres.immediate_poweroff() instead. Power sequencing
                // requirements have changed since this was created, this routine does not actually cut power
                // anymore.
                com.txrx(ComState::CHG_BOOST_OFF.verb);
                com.txrx(ComState::LINK_SET_INTMASK.verb);
                com.txrx(0); // suppress interrupts on suspend

                if bl_main != 0 || bl_sec != 0 {
                    com.txrx(ComState::BL_START.verb); // this will turn off the backlights
                }
                info!("power off called");
                com.txrx(ComState::POWER_OFF.verb);
                com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)); // consume the obligatory return value, even if not used
            }
            Some(Opcode::BoostOff) => {
                com.txrx(ComState::CHG_BOOST_OFF.verb);
            }
            Some(Opcode::BoostOn) => {
                com.txrx(ComState::CHG_BOOST_ON.verb);
            }
            Some(Opcode::SetBackLight) => msg_scalar_unpack!(msg, main, secondary, _, _, {
                #[cfg(not(target_os = "xous"))]
                log::info!("HOSTED: set backlight to {},{}", main, secondary);
                bl_main = main;
                bl_sec = secondary;
                com.txrx(ComState::BL_START.verb | (main as u16) & 0x1f | (((secondary as u16) & 0x1f) << 5));
            }),
            Some(Opcode::ImuAccelReadBlocking) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::GYRO_UPDATE.verb);
                com.txrx(ComState::GYRO_READ.verb);
                let x = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                let y = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                let z = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                let id = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                xous::return_scalar2(
                    msg.sender,
                    ((x as usize) << 16) | y as usize,
                    ((z as usize) << 16) | id as usize,
                )
                .expect("coludn't return accelerometer read data");
            }),
            Some(Opcode::BattStats) => {
                let stats = com.get_battstats();
                let raw_stats: [usize; 2] = stats.into();
                xous::return_scalar2(msg.sender, raw_stats[0], raw_stats[1])
                    .expect("couldn't return batt stats request");
            }
            Some(Opcode::MoreStats) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let stats = com.get_more_stats();
                buffer.replace(stats).unwrap();
            }
            Some(Opcode::PollUsbCc) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                let usb_cc: [u32; 2] = com.poll_usb_cc();
                xous::return_scalar2(msg.sender, usb_cc[0] as usize, usb_cc[1] as usize)
                    .expect("couldn't return Usb CC result");
            }),
            Some(Opcode::BattStatsNb) => {
                for &maybe_conn in battstats_conns.iter() {
                    if let Some(conn) = maybe_conn {
                        com.workqueue.push(WorkRequest { work: ComState::STAT, sender: conn });
                        //.unwrap();
                    }
                }
            }
            Some(Opcode::ShipMode) => {
                com.txrx(ComState::POWER_SHIPMODE.verb);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack ship mode");
            }
            Some(Opcode::Wf200Rev) => {
                com.txrx(ComState::WFX_FW_REV_GET.verb);
                let major = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                let minor = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                let build = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                xous::return_scalar(
                    msg.sender,
                    ((major as usize) << 16) | ((minor as usize) << 8) | (build as usize),
                )
                .expect("couldn't return WF200 firmware rev");
            }
            Some(Opcode::EcGitRev) => {
                com.txrx(ComState::EC_GIT_REV.verb);
                let rev_msb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                let rev_lsb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                let dirty = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                ec_git_rev = ((rev_msb as u32) << 16) | (rev_lsb as u32);
                xous::return_scalar2(msg.sender, ec_git_rev as usize, dirty as usize)
                    .expect("couldn't return WF200 firmware rev");
            }
            Some(Opcode::EcSwTag) => {
                if ec_git_rev == LEGACY_REV {
                    // this corresponds to a 0.9.5 tag -- the last tag shipped that lacked detailed versioning
                    xous::return_scalar(msg.sender, 0x00_09_05_00)
                        .expect("couldn't return WF200 revision tag");
                } else {
                    ec_tag = parse_version(&mut com);
                    xous::return_scalar(msg.sender, ec_tag as usize)
                        .expect("couldn't return WF200 revision tag");
                }
            }
            Some(Opcode::Wf200Reset) => {
                let start = ticktimer.elapsed_ms();
                com.txrx(ComState::WF200_RESET.verb);
                com.txrx(0);
                if ec_tag >= u32::from_be_bytes(ComState::LINK_PING.apilevel) {
                    let mut attempts = 0;
                    let ping_value = 0xaeedu16; // for various reasons, this is a string that is "unlikely" to happen randomly
                    loop {
                        com.txrx(ComState::LINK_PING.verb);
                        com.txrx(ping_value);
                        let pong = com.wait_txrx(ComState::LINK_READ.verb, Some(5000)); // should finish with 5 seconds
                        let phase = com.wait_txrx(ComState::LINK_READ.verb, Some(500));
                        if pong == !ping_value && phase == 0x600d {
                            // 0x600d is a hard-coded constant. It's included to confirm that we aren't
                            // "wedged" just sending one value back at us
                            break;
                        } else {
                            log::warn!(
                                "Wf200 reset: establishing link sync, attempt {} [{:04x}/{:04x}]",
                                attempts,
                                pong,
                                phase
                            );
                            com.txrx(ComState::LINK_SYNC.verb);
                            ticktimer.sleep_ms(200).unwrap();
                            attempts += 1;
                        }
                        if attempts > 50 {
                            // something has gone horribly wrong. Reset the EC entirely.
                            llio.ec_reset().unwrap();
                            ticktimer.sleep_ms(EC_BOOT_WAIT_MS).unwrap();
                            com.txrx(ComState::TRNG_SEED.verb);
                            for _ in 0..2 {
                                let mut rng = trng.get_u64().expect("couldn't fetch rngs");
                                for _ in 0..4 {
                                    com.txrx(rng as u16);
                                    rng >>= 16;
                                }
                            }
                            break;
                        }
                    }
                } else {
                    // replace with a dead wait loop for older revs
                    log::warn!("EC rev is too old: replacing EC RESET SYNC with a dead wait loop");
                    ticktimer.sleep_ms(7000).unwrap();
                }
                xous::return_scalar(msg.sender, (ticktimer.elapsed_ms() - start) as usize).unwrap();
            }
            Some(Opcode::Wf200Disable) => {
                com.txrx(ComState::WF200_RESET.verb);
                com.txrx(1);
            }
            Some(Opcode::ScanOn) => {
                com.txrx(ComState::SSID_SCAN_ON.verb);
            }
            Some(Opcode::ScanOff) => {
                com.txrx(ComState::SSID_SCAN_OFF.verb);
            }
            Some(Opcode::SsidCheckUpdate) => {
                com.txrx(ComState::SSID_CHECK.verb);
                let available = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                xous::return_scalar(msg.sender, available as usize).expect("couldn't return SsidCheckUpdate");
            }
            Some(Opcode::SsidFetchAsString) => {
                if ec_tag == LEGACY_TAG {
                    // this is kept around because the original firmware shipped with units don't support
                    // software tagging and the SSID API was modified. This allows the SOC
                    // to interop with older versions of the EC for SSID scanning. If it's
                    // 2023 and you're looking at this comment and thinking about removing this code, it might
                    // actually be ok to do that. Just check that the factory test
                    // infrastructure is fully updated to match a modern EC rev.
                    use core::fmt::Write;
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };

                    com.txrx(ComState::SSID_FETCH.verb);
                    let mut ssid_list: [[u8; 32]; 6] = [[0; 32]; 6]; // index as ssid_list[6][32]
                    for i in 0..16 * 6 {
                        let data = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                        let lsb: u8 = (data & 0xff) as u8;
                        let msb: u8 = ((data >> 8) & 0xff) as u8;
                        //if lsb == 0 { lsb = 0x20; }
                        //if msb == 0 { msb = 0x20; }
                        ssid_list[i / 16][(i % 16) * 2] = lsb;
                        ssid_list[i / 16][(i % 16) * 2 + 1] = msb;
                    }
                    // this is a questionably useful return format -- maybe it's actually more useful to
                    // return the raw characters?? for now, this is good enough for human
                    // eyes, but a scrollable list of SSIDs might be more useful with the raw u8
                    // representation
                    let mut ssid_str = String::from("Top 6 SSIDs:\n");
                    let mut itemized = String::new();
                    for i in 0..6 {
                        let mut stop = 0;
                        // truncate the nulls
                        for l in 0..ssid_list[i].len() {
                            stop = l;
                            if ssid_list[i][l] == 0 {
                                break;
                            }
                        }
                        let ssid = core::str::from_utf8(&ssid_list[i][0..stop]);
                        match ssid {
                            Ok(textid) => {
                                itemized.clear();
                                write!(itemized, "{}. {}\n", i + 1, textid).unwrap();
                                ssid_str.push_str(&itemized);
                            }
                            _ => {
                                ssid_str.push_str("-Parse Error-\n");
                            }
                        }
                    }
                    buffer.replace(ssid_str).unwrap();
                } else {
                    log::error!("This API is not implemented for this EC firmware revision");
                }
            }
            Some(Opcode::SsidFetchAsStringV2) => {
                if ec_tag == LEGACY_TAG {
                    log::error!("This API is not implemented for legacy EC revs");
                    continue;
                }
                com.txrx(ComState::SSID_FETCH_STR.verb);
                // these sizes are hard-coded constants from the EC firmware. We don't have a good cross-code
                // base method for sharing these yet, so they are just magic numbers.
                let mut ssid_list: [[u8; 34]; 8] = [[0; 34]; 8];
                for record in ssid_list.iter_mut() {
                    for word in record.chunks_mut(2) {
                        let data = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                        word[0] = (data & 0xff) as u8;
                        word[1] = ((data >> 8) & 0xff) as u8;
                    }
                }
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut ssid_ret =
                    buffer.to_original::<SsidReturn, _>().expect("couldn't convert incoming storage");
                for (raw, ssid_rec) in ssid_list.iter().zip(ssid_ret.list.iter_mut()) {
                    ssid_rec.rssi = raw[0];
                    let len = if raw[1] < 32 { raw[1] as usize } else { 32 };
                    let ssid_str =
                        core::str::from_utf8(&raw[2..2 + len as usize]).unwrap_or("UTF-8 parse error");
                    ssid_rec.name.clear(); // should be pre-cleared, but let's just be safe about it
                    ssid_rec.name.push_str(ssid_str);
                }
                buffer.replace(ssid_ret).unwrap();
            }
            Some(Opcode::WlanOn) => {
                com.txrx(ComState::WLAN_ON.verb);
                // re-sync the link, because the COM will take about a second to reload the Wifi drivers
                #[cfg(not(any(windows, unix)))] // avoid errors in hosted mode
                {
                    ticktimer.sleep_ms(100).unwrap(); // give the EC a moment to de-chatter
                    let mut attempts = 0;
                    let ping_value = 0xaeedu16; // for various reasons, this is a string that is "unlikely" to happen randomly
                    loop {
                        com.txrx(ComState::LINK_PING.verb);
                        com.txrx(ping_value);
                        let pong = com.wait_txrx(ComState::LINK_READ.verb, Some(150)); // this should "stall" until the EC comes out of reset
                        let phase = com.wait_txrx(ComState::LINK_READ.verb, Some(150));
                        if pong == !ping_value && phase == 0x600d {
                            // 0x600d is a hard-coded constant. It's included to confirm that we aren't
                            // "wedged" just sending one value back at us
                            log::info!("Wifi on: link established");
                            break;
                        } else {
                            log::info!(
                                "Wifi on: establishing link sync, attempt {} [{:04x}/{:04x}]",
                                attempts,
                                pong,
                                phase
                            );
                            com.txrx(ComState::LINK_SYNC.verb);
                            ticktimer.sleep_ms(200).unwrap();
                            attempts += 1;
                        }
                        if attempts > 50 {
                            log::error!(
                                "EC didn't sync after wifi on...continuing and praying for the best."
                            );
                            break;
                        }
                    }
                }
                xous::return_scalar(msg.sender, 1).ok();
            }
            Some(Opcode::WlanOff) => {
                com.txrx(ComState::WLAN_OFF.verb);
            }
            Some(Opcode::WlanRssi) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::WLAN_GET_RSSI.verb);
                let maybe_rssi = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                // raw code sent, error interpretation done by lib function
                xous::return_scalar(msg.sender, maybe_rssi as usize).unwrap();
            }),
            Some(Opcode::WlanSyncState) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::WLAN_SYNC_STATE.verb);
                let link = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                let dhcp = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                xous::return_scalar2(msg.sender, link as usize, dhcp as usize).unwrap();
            }),
            Some(Opcode::WlanSetSSID) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let ssid = buffer.to_original::<String, _>().unwrap();
                info!("WlanSetSSID: {}", ssid);
                let mut str_ser = StringSer::<STR_32_WORDS>::new();
                match str_ser.encode(&ssid) {
                    Ok(tx_words) => {
                        com.txrx(ComState::WLAN_SET_SSID.verb);
                        for w in tx_words.iter() {
                            com.txrx(*w);
                        }
                    }
                    _ => info!("WlanSetSSID FAIL"),
                }
            }
            Some(Opcode::WlanSetPass) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let pass = buffer.to_original::<String, _>().unwrap();
                info!("WlanSetPass *ssssh!*");
                let mut str_ser = StringSer::<STR_64_WORDS>::new();
                match str_ser.encode(&pass) {
                    Ok(tx_words) => {
                        com.txrx(ComState::WLAN_SET_PASS.verb);
                        for w in tx_words.iter() {
                            com.txrx(*w);
                        }
                    }
                    _ => info!("WlanSetPASS FAIL"),
                }
            }
            Some(Opcode::WlanJoin) => {
                info!("Sent WlanJoin");
                com.txrx(ComState::WLAN_JOIN.verb);
            }
            Some(Opcode::WlanLeave) => {
                info!("Sent WlanLeave");
                com.txrx(ComState::WLAN_LEAVE.verb);
            }
            Some(Opcode::WlanStatus) => {
                if ec_tag <= LEGACY_TAG {
                    log::warn!("Legacy EC detected. Ignoring status request update");
                } else {
                    let mut buffer =
                        unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                    com.txrx(ComState::WLAN_BIN_STATUS.verb);
                    let maybe_rssi = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    let rssi = if (maybe_rssi >> 8) & 0xff != 0 {
                        None
                    } else {
                        // rssi as reported is a number from 0-110, where 110 is 0 dbm.
                        // a perhaps dubious decision was made to shift the reported value over by one before
                        // returning to the host, so we lost a 0.5dBm step. But...not a big deal, and not
                        // worth putting every user through an EC update to gain some
                        // resolution they never see. note there is also a function in
                        // lib.rs/wlan_get_rssi() that has to be patched if
                        // this is fixed.
                        Some((110 - maybe_rssi) & 0xff)
                    };
                    let link_state = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    let mut ipv4_raw = Ipv4Conf::default().encode_u16();
                    for dest in ipv4_raw.iter_mut() {
                        *dest = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    }
                    let mut ssid_buf = [0u8; 34];
                    for w in ssid_buf.chunks_mut(2) {
                        let word = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)).to_le_bytes();
                        w[0] = word[0];
                        w[1] = word[1];
                    }
                    let ssid_len = u16::from_le_bytes(ssid_buf[0..2].try_into().unwrap()) as usize;
                    let ssid_checked_len = if ssid_len < 32 { ssid_len } else { 32 };
                    let ssid_str =
                        core::str::from_utf8(&ssid_buf[2..2 + ssid_checked_len]).unwrap_or("Disconnected");
                    let status = WlanStatusIpc {
                        ssid: if let Some(rssi) = rssi {
                            log::debug!("RSSI: -{}dBm", rssi);
                            Some(SsidRecord { rssi: rssi as u8, name: String::from(ssid_str) })
                        } else {
                            None
                        },
                        link_state,
                        ipv4: ipv4_raw,
                    };
                    buffer.replace(status).unwrap();
                }
            }
            Some(Opcode::WlanGetConfig) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };

                com.txrx(ComState::WLAN_GET_IPV4_CONF.verb);
                let mut prealloc = Ipv4Conf::default().encode_u16();
                for dest in prealloc.iter_mut() {
                    *dest = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                }
                #[cfg(not(target_os = "xous"))]
                {
                    // assign a fake MAC address in hosted mode so we don't crash smoltcp
                    for i in 1..4 {
                        prealloc[i] = i as u16 - 1;
                    }
                }
                /*
                    ret[1] = self.mac[0] as u16 | (self.mac[1] as u16) << 8;
                    ret[2] = self.mac[2] as u16 | (self.mac[3] as u16) << 8;
                    ret[3] = self.mac[4] as u16 | (self.mac[5] as u16) << 8;
                    multicast is self.mac[0] & 0x1 == 1 -> this gets set when 0xDDDD or 0xDEAD is transmitted as EC is in reset
                    broadcast is 0xFF's -> this is unlikely to be sent, it only is sent if the EC won't configure at all
                */
                // punting this to the lib side of things, so we can return an error and not just a bogus
                // value. prealloc[1] &= 0xFF_FE; // this ensures the "safety" of a reported
                // MAC, even if the EC is broken and avoiding system panic (issue #152)
                buffer.replace(prealloc).expect("couldn't return result on FlashOp");
            }
            Some(Opcode::WlanFetchPacket) => {
                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let mut retbuf = buffer.to_original::<[u8; NET_MTU], _>().unwrap();
                let be_bytes: [u8; 2] = [retbuf[0], retbuf[1]];
                let len_bytes = u16::from_be_bytes(be_bytes);
                let len_words = if len_bytes % 2 == 0 { len_bytes / 2 } else { len_bytes / 2 + 1 };
                if len_bytes > NET_MTU as u16 {
                    log::error!("invalid packet fetch length: {}, aborting without fetch", len_bytes);
                    continue;
                }
                com.txrx(ComState::NET_FRAME_FETCH_0.verb | len_bytes);
                for word_index in 0..len_words as usize {
                    let w = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    let be_bytes = w.to_be_bytes();
                    retbuf[word_index * 2] = be_bytes[0];
                    retbuf[word_index * 2 + 1] = be_bytes[1];
                }
                log::trace!("rx: {:?}", &retbuf[..len_bytes as usize]);
                buffer.replace(retbuf).expect("couldn't return packet");
            }
            Some(Opcode::WlanSendPacket) => {
                let buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let txbuf = buffer.as_flat::<[u8; NET_MTU + 2], _>().unwrap();
                let be_bytes: [u8; 2] = [txbuf[0], txbuf[1]];
                let len_bytes = u16::from_be_bytes(be_bytes);
                let len_words = if len_bytes % 2 == 0 { len_bytes / 2 } else { len_bytes / 2 + 1 };
                if len_bytes > NET_MTU as u16 {
                    log::error!("invalid packet send length: {}, aborting without send", len_bytes);
                    continue;
                }
                log::trace!("tx: {:?}", &txbuf[2..len_bytes as usize + 2]);
                com.txrx(ComState::NET_FRAME_SEND_0.verb | len_bytes);
                for word_index in 0..len_words as usize {
                    let be_bytes: [u8; 2] = [txbuf[2 + word_index * 2], txbuf[2 + word_index * 2 + 1]];
                    com.txrx(u16::from_be_bytes(be_bytes));
                }
            }
            Some(Opcode::IntSetMask) => msg_blocking_scalar_unpack!(msg, mask_val, _, _, _, {
                desired_int_mask = mask_val as u16;
                com.txrx(ComState::LINK_SET_INTMASK.verb);
                com.txrx(mask_val as u16);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack IntSetMask");
            }),
            Some(Opcode::IntAck) => msg_blocking_scalar_unpack!(msg, ack_val, _, _, _, {
                com.txrx(ComState::LINK_ACK_INTERRUPT.verb);
                com.txrx(ack_val as u16);
                xous::return_scalar(msg.sender, 1).expect("couldn't ack IntAck");
            }),
            Some(Opcode::IntGetMask) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::LINK_GET_INTMASK.verb);
                let mask = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                xous::return_scalar(msg.sender, mask as usize).expect("couldn't return mask value");
            }),
            Some(Opcode::IntFetchVector) => msg_blocking_scalar_unpack!(msg, _, _, _, _, {
                com.txrx(ComState::LINK_GET_INTERRUPT.verb);
                let maybe_vector = com.try_wait_txrx(ComState::LINK_READ.verb, 500); // longer timeout because interrupts tend to be busy times
                let rxlen = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                if let Some(vector) = maybe_vector {
                    log::debug!("vector: 0x{:x}, len: {}", vector, rxlen);
                    xous::return_scalar2(msg.sender, vector as _, rxlen as _)
                        .expect("couldn't return IntFetchVector");
                } else {
                    log::error!(
                        "Timeout during interrupt vector fetch. EC may not be responsive. Returning error vector...."
                    );
                    xous::return_scalar2(msg.sender, com_rs::INT_INVALID as usize, 0)
                        .expect("couldn't return IntFetchVector");
                }
            }),
            Some(Opcode::WlanDebug) => {
                com.txrx(ComState::WLAN_GET_ERRCOUNTS.verb);
                let mut tx_errs_16 = [0u16; 2];
                let mut drops_16 = [0u16; 2];
                tx_errs_16[0] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                tx_errs_16[1] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                drops_16[0] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                drops_16[1] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));

                let mut config_16 = [0u16; 2];
                let mut alloc_fail_16 = [0u16; 2];
                let mut alloc_oversize_16 = [0u16; 2];
                let mut control = 0;
                let mut alloc_free_count = 0;
                if ec_tag >= u32::from_be_bytes(ComState::WF200_DEBUG.apilevel) {
                    com.txrx(ComState::WF200_DEBUG.verb);
                    config_16[0] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    config_16[1] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    control = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    alloc_fail_16[0] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    alloc_fail_16[1] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    alloc_oversize_16[0] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    alloc_oversize_16[1] = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                    alloc_free_count = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                }

                let mut buffer =
                    unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let debug = WlanDebug {
                    tx_errs: from_le_words(tx_errs_16),
                    drops: from_le_words(drops_16),
                    config: from_le_words(config_16),
                    control,
                    alloc_fail: from_le_words(alloc_fail_16),
                    alloc_oversize: from_le_words(alloc_oversize_16),
                    alloc_free_count,
                };
                buffer.replace(debug).unwrap();
            }
            None => {
                error!("unknown opcode");
                break;
            }
        }

        if let Some(dropped_cid) = com.process_queue() {
            for entry in battstats_conns.iter_mut() {
                if let Some(cid) = *entry {
                    if cid == dropped_cid {
                        *entry = None;
                        break;
                    }
                }
            }
        }
    }
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(com_sid).unwrap();
    xous::destroy_server(com_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}

fn parse_version(com: &mut crate::implementation::XousCom) -> u32 {
    use xous_semver::SemVer;

    // this feature was only introduced since 0.9.6. Unfortunately, there is no good way to figure out
    // if we support it for earlier versions of the EC.
    com.txrx(ComState::EC_SW_TAG.verb);
    let mut rev_ret = [0u16; ComState::EC_SW_TAG.r_words as usize];
    for w in rev_ret.iter_mut() {
        *w = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
    }
    // unpack u16 words into u8 array
    let mut rev_bytes = [0u8; (ComState::EC_SW_TAG.r_words as usize - 1) * 2];
    for (src, dst) in rev_ret[1..].iter().zip(rev_bytes.chunks_mut(2)) {
        dst[0] = src.to_le_bytes()[0];
        dst[1] = src.to_le_bytes()[1];
    }
    // translate u8 array into &str
    let len_checked =
        if rev_ret[0] as usize <= rev_bytes.len() { rev_ret[0] as usize } else { rev_bytes.len() };
    if len_checked < 2 {
        // something is very wrong if our length is too short
        return 0;
    }
    let revstr = core::str::from_utf8(&rev_bytes[..len_checked]).unwrap_or("v0.9.5-0"); // fake version number for hosted mode, equal to a very old version of the EC
    let ver = SemVer::from_str(revstr).unwrap_or(SemVer::from_str("v0.9.5-1").unwrap());
    (ver.extra & 0xff) as u32
        | ((ver.rev & 0xff) as u32) << 8
        | (((ver.min & 0xff) as u32) << 16)
        | (((ver.maj & 0xff) as u32) << 24)
}

fn from_le_words(words: [u16; 2]) -> u32 { words[0] as u32 | (words[1] as u32) << 16 }
