#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::Opcode;

use num_traits::{ToPrimitive, FromPrimitive};

use log::{error, info, trace};

use com_rs_ref as com_rs;
use com_rs::*;
use com_rs::serdes::{STR_32_WORDS, STR_64_WORDS, STR_64_U8_SIZE, StringSer, StringDes};

use xous::{CID, msg_scalar_unpack, msg_blocking_scalar_unpack};
use xous_ipc::{Buffer, String};

const STD_TIMEOUT: u32 = 100;
#[derive(Debug, Copy, Clone)]
pub struct WorkRequest {
    work: ComSpec,
    sender: CID,
}

fn return_battstats(cid: CID, stats: api::BattStats) -> Result<(), xous::Error> {
    let rawstats: [usize; 2] = stats.into();
    xous::send_message(cid,
        xous::Message::new_scalar(api::Callback::BattStats.to_usize().unwrap(),
        rawstats[0], rawstats[1], 0, 0)
    ).map(|_| ())
}

#[cfg(any(target_os = "none", target_os = "xous"))]
mod implementation {
    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;
    use com_rs_ref as com_rs;
    use com_rs::*;
    use log::error;
    use utralib::generated::*;
    use susres::{RegManager, RegOrField, SuspendResume};

    const STD_TIMEOUT: u32 = 100;

    pub struct XousCom {
        csr: utralib::CSR<u32>,
        susres: RegManager::<{utra::com::COM_NUMREGS}>,
        ticktimer: ticktimer_server::Ticktimer,
        pub workqueue: Vec<WorkRequest>,
        busy: bool,
        stby_current: Option<i16>,
    }

    fn handle_irq(_irq_no: usize, arg: *mut usize) {
        let xc = unsafe { &mut *(arg as *mut XousCom) };
        // just clear the pending request, as this is used as a "wait" until request function
        xc.csr
            .wo(utra::com::EV_PENDING, xc.csr.r(utra::com::EV_PENDING));
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

            let mut xc = XousCom {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                susres: RegManager::new(csr.as_mut_ptr() as *mut u32),
                ticktimer,
                workqueue: Vec::new(),
                busy: false,
                stby_current: None,
            };

            xous::claim_interrupt(
                utra::com::COM_IRQ,
                handle_irq,
                (&mut xc) as *mut XousCom as *mut usize,
            )
            .expect("couldn't claim irq");

            xc.susres.push(RegOrField::Reg(utra::com::CONTROL), None);
            xc.susres.push_fixed_value(RegOrField::Reg(utra::com::EV_PENDING), 0xFFFF_FFFF);
            xc.susres.push(RegOrField::Reg(utra::com::EV_ENABLE), None);

            xc
        }
        pub fn suspend(&mut self) {
            self.susres.suspend();
            self.csr.wo(utra::com::EV_ENABLE, 0);
        }
        pub fn resume(&mut self) {
            self.susres.resume();
            // issue a "link sync" command because the COM had continued running, and we may have sent garbage during suspend
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
                    if (self.ticktimer.elapsed_ms() - curtime) > to
                    {
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
                        },
                        Ok(()) => None,
                        _ => panic!("unhandled error in callback process_queue"),
                    }
                } else {
                    error!(
                        "unimplemented work queue responder 0x{:x}",
                        work_descriptor.work.verb
                    );
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
                ret[1] as u32 | ((ret[2] as u32) << 16)
            ]
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod implementation {
    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;
    use com_rs_ref as com_rs;
    use com_rs::*;
    use log::error;

    pub struct XousCom {
        pub workqueue: Vec<WorkRequest>,
        busy: bool,
    }

    impl XousCom {
        pub fn new() -> XousCom {
            XousCom {
                workqueue: Vec::new(),
                busy: false,
            }
        }
        pub fn suspend(&self) {}
        pub fn resume(&self) {}

        pub fn txrx(&mut self, _tx: u16) -> u16 {
            0xDEAD as u16
        }

        pub fn wait_txrx(&mut self, _tx: u16, _timeout: Option<u32>) -> u16 {
            0xDEAD as u16
        }

        pub fn get_battstats(&mut self) -> BattStats {
            BattStats {
                voltage: 3700,
                current: -150,
                soc: 50,
                remaining_capacity: 750,
            }
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
                        },
                        Ok(()) => None,
                        _ => panic!("unhandled error in callback process_queue"),
                    }
                } else {
                    error!(
                        "unimplemented work queue responder 0x{:x}",
                        work_descriptor.work.verb
                    );
                    None
                };
                self.busy = false;
                ret
            } else {
                None
            }
        }
        pub fn get_more_stats(&mut self) -> [u16; 15] {
            [0; 15]
        }

        pub fn poll_usb_cc(&mut self) -> [u32; 2] {
            [0; 2]
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::XousCom;

    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // unlimited connections allowed -- any server is currently allowed to talk to COM. This might need to be revisited.
    let com_sid = xns.register_name(api::SERVER_NAME_COM, None).expect("can't register server");
    trace!("registered with NS -- {:?}", com_sid);

    // Create a new com object
    let mut com = XousCom::new();

    // register a suspend/resume listener
    let sr_cid = xous::connect(com_sid).expect("couldn't create suspend callback connection");
    let mut susres = susres::Susres::new(&xns, Opcode::SuspendResume as u32, sr_cid).expect("couldn't create suspend/resume object");

    // create an array to track return connections for battery stats
    let mut battstats_conns: [Option<xous::CID>; 32] = [None; 32];
    // other future notification vectors shall go here

    let mut bl_main = 0;
    let mut bl_sec = 0;

    let mut flash_id: Option<[u32;4]> = None; // only one process can acquire this, and its ID is stored here.
    const FLASH_LEN: u32 = 0x10_0000;
    const FLASH_TIMEOUT: u32 = 250;

    trace!("starting main loop");
    loop {
        let mut msg = xous::receive_message(com_sid).unwrap();
        trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::SuspendResume) => xous::msg_scalar_unpack!(msg, token, _, _, _, {
                if bl_main != 0 || bl_sec != 0 {
                    com.txrx(ComState::BL_START.verb); // this will turn off the backlights
                }
                com.suspend();
                susres.suspend_until_resume(token).expect("couldn't execute suspend/resume");
                com.resume();
                if bl_main != 0 || bl_sec != 0 { // restore the backlight settings, if they are not 0
                    com.txrx(ComState::BL_START.verb | (bl_main as u16) & 0x1f | (((bl_sec as u16) & 0x1f) << 5));
                }
            }),
            Some(Opcode::FlashAcquire) => msg_blocking_scalar_unpack!(msg, id0, id1, id2, id3, {
                let acquired = if flash_id.is_none() {
                    flash_id = Some([id0 as u32, id1 as u32, id2 as u32, id3 as u32]);
                    1
                } else {
                    0
                };
                xous::return_scalar(msg.sender, acquired as usize).expect("couldn't acknowledge acquire message");
            }),
            Some(Opcode::FlashOp) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
                let flash_op = buffer.to_original::<api::FlashRecord, _>().unwrap();
                let mut pass = false;
                if let Some(id) = flash_id {
                    if id == flash_op.id {
                        match flash_op.op {
                            api::FlashOp::Erase(addr, len) => {
                                if addr < FLASH_LEN && len + addr < FLASH_LEN {
                                    log::debug!("Erasing EC region starting at 0x{:x}, lenth 0x{:x}", addr, len);
                                    com.txrx(ComState::FLASH_ERASE.verb);
                                    com.txrx((addr >> 16) as u16);
                                    com.txrx(addr as u16);
                                    com.txrx((len >> 16) as u16);
                                    com.txrx(len as u16);
                                    while ComState::FLASH_ACK.verb != com.wait_txrx(ComState::FLASH_WAITACK.verb, Some(FLASH_TIMEOUT)) {
                                        xous::yield_slice();
                                    }
                                    pass = true;
                                } else {
                                    pass = false;
                                }
                            },
                            api::FlashOp::Program(addr, some_pages) => {
                                com.txrx(ComState::FLASH_LOCK.verb);
                                let mut prog_ptr = addr;
                                // this will fill the 1280-deep FIFO with up to 4 pages of data for programming
                                pass = true;
                                for &maybe_page in some_pages.iter() {
                                    if prog_ptr < FLASH_LEN - 256 {
                                        log::trace!("Prog EC page at 0x{:x}", prog_ptr);
                                        if let Some(page) = maybe_page {
                                            com.txrx(ComState::FLASH_PP.verb);
                                            com.txrx((prog_ptr >> 16) as u16);
                                            com.txrx(prog_ptr as u16);
                                            for i in 0..128 {
                                                com.txrx(page[i*2] as u16 | ((page[i*2+1] as u16) << 8));
                                            }
                                        }
                                        prog_ptr += 256;
                                    } else {
                                        pass = false;
                                    }
                                }
                                // wait for completion only after all 4 pages are sent
                                while ComState::FLASH_ACK.verb != com.wait_txrx(ComState::FLASH_WAITACK.verb, Some(FLASH_TIMEOUT)) {
                                    xous::yield_slice();
                                }
                                com.txrx(ComState::FLASH_UNLOCK.verb);
                            }
                        }
                    }
                } else {
                    pass = false;
                }
                let response = if pass {
                    api::FlashResult::Pass
                } else {
                    api::FlashResult::Fail
                };
                buffer.replace(response).expect("couldn't return result on FlashOp");
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
                }
            ),
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
                let l = buffer.to_original::<String::<512>, _>().unwrap();
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
                    if (i * 2 + 1) == length as usize { // odd last element
                        word = line[i * 2] as u16;
                    } else if i * 2 < length as usize {
                        word = (line[i*2] as u16) | ((line[i*2+1] as u16) << 8);
                    } else {
                        word = 0;
                    }
                    com.txrx(word);
                    //info!("0x{:04x}", word);
                }
            }
            Some(Opcode::PowerOffSoc) => {
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
                xous::return_scalar2(msg.sender,
                    ((x as usize) << 16) | y as usize,
                    ((z as usize) << 16) | id as usize
                ).expect("coludn't return accelerometer read data");
            }),
            Some(Opcode::BattStats) => {
                let stats = com.get_battstats();
                let raw_stats: [usize; 2] = stats.into();
                xous::return_scalar2(msg.sender, raw_stats[0], raw_stats[1])
                    .expect("couldn't return batt stats request");
            }
            Some(Opcode::MoreStats) => {
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
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
                        com.workqueue
                            .push(WorkRequest {
                                work: ComState::STAT,
                                sender: conn,
                            });
                            //.unwrap();
                    }
                }
            }
            Some(Opcode::ShipMode) => {
                com.txrx(ComState::POWER_SHIPMODE.verb);
                xous::return_scalar(msg.sender, 1)
                    .expect("couldn't ack ship mode");
            }
            Some(Opcode::Wf200Rev) => {
                com.txrx(ComState::WFX_FW_REV_GET.verb);
                let major = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                let minor = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                let build = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                xous::return_scalar(
                    msg.sender,
                    ((major as usize) << 16) | ((minor as usize) << 8) | (build as usize)
                )
                .expect("couldn't return WF200 firmware rev");
            }
            Some(Opcode::EcGitRev) => {
                com.txrx(ComState::EC_GIT_REV.verb);
                let rev_msb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                let rev_lsb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                let dirty = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                xous::return_scalar2(
                    msg.sender,
                    ((rev_msb as usize) << 16) | (rev_lsb as usize),
                    dirty as usize
                )
                .expect("couldn't return WF200 firmware rev");
            }
            Some(Opcode::Wf200Reset) => {
                com.txrx(ComState::WF200_RESET.verb);
                com.txrx(0);
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
                xous::return_scalar(
                    msg.sender, available as usize
                ).expect("couldn't return SsidCheckUpdate");
            },
            Some(Opcode::SsidFetchAsString) => {
                use core::fmt::Write;
                let mut buffer = unsafe { Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };

                com.txrx(ComState::SSID_FETCH.verb);
                let mut ssid_list: [[u8; 32]; 6] = [[0; 32]; 6]; // index as ssid_list[6][32]
                for i in 0..16 * 6 {
                    let data = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                    let lsb : u8 = (data & 0xff) as u8;
                    let msb : u8 = ((data >> 8) & 0xff) as u8;
                    //if lsb == 0 { lsb = 0x20; }
                    //if msb == 0 { msb = 0x20; }
                    ssid_list[i / 16][(i % 16) * 2] = lsb;
                    ssid_list[i / 16][(i % 16) * 2 + 1] = msb;
                }
                // this is a questionably useful return format -- maybe it's actually more useful to return the raw characters??
                // for now, this is good enough for human eyes, but a scrollable list of SSIDs might be more useful with the raw u8 representation
                let mut ssid_str = xous_ipc::String::<256>::from_str("Top 6 SSIDs:\n");
                let mut itemized = xous_ipc::String::<256>::new();
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
                            write!(itemized, "{}. {}\n", i+1, textid).unwrap();
                            ssid_str.append(itemized.as_str().unwrap()).unwrap();
                        },
                        _ => {
                            ssid_str.append("-Parse Error-\n").unwrap();
                        },
                    }
                }
                buffer.replace(ssid_str).unwrap();
            }
            Some(Opcode::WlanOn) => {
                info!("TODO: implement WlanOn");
                com.txrx(ComState::WLAN_ON.verb);
            }
            Some(Opcode::WlanOff) => {
                info!("TODO: implement WlanOff");
                com.txrx(ComState::WLAN_OFF.verb);
            }
            Some(Opcode::WlanSetSSID) => {
                const WF200_SSID_LEN: usize = 32;
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let ssid = buffer.to_original::<String::<WF200_SSID_LEN>, _>().unwrap();
                info!("WlanSetSSID: {}", ssid);
                let mut str_ser = StringSer::<STR_32_WORDS>::new();
                match str_ser.encode(&ssid.to_str()) {
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
                const WF200_PASS_LEN: usize = 64;
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let pass = buffer.to_original::<String::<WF200_PASS_LEN>, _>().unwrap();
                info!("WlanSetPass: {}", pass);
                let mut str_ser = StringSer::<STR_64_WORDS>::new();
                match str_ser.encode(&pass.to_str()) {
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
                info!("TODO: implement WlanJoin");
                com.txrx(ComState::WLAN_JOIN.verb);
            }
            Some(Opcode::WlanLeave) => {
                info!("TODO: implement WlanLeave");
                com.txrx(ComState::WLAN_LEAVE.verb);
            }
            Some(Opcode::WlanStatus) => {
                let mut buffer = unsafe {
                    Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
                };
                com.txrx(ComState::WLAN_STATUS.verb);
                let mut rx_buf: [u16; STR_64_WORDS] = [0; STR_64_WORDS];
                for dest in rx_buf.iter_mut() {
                    *dest = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                }
                let mut des = StringDes::<STR_64_WORDS, STR_64_U8_SIZE>::new();
                match des.decode_u16(&rx_buf) {
                    Ok(status) => {
                        info!("status: {}", status);
                        let status_str = String::<STR_64_U8_SIZE>::from_str(&status);
                        let _ = buffer.replace(status_str);
                    }
                    _ => info!("status decode failed"),
                };
            }
            None => {error!("unknown opcode"); break},
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
