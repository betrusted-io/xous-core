#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::Opcode;

use core::convert::TryFrom;
use num_traits::{ToPrimitive, FromPrimitive};

use log::{error, info, trace};

use com_rs_ref as com_rs;
use com_rs::*;

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

#[cfg(target_os = "none")]
mod implementation {
    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;
    use com_rs_ref as com_rs;
    use com_rs::*;
    use log::error;
    use utralib::generated::*;
    use xous::CID;

    use heapless::Vec;
    use heapless::consts::U64;

    const STD_TIMEOUT: u32 = 100;

    pub struct XousCom {
        csr: utralib::CSR<u32>,
        ticktimer: ticktimer_server::Ticktimer,
        pub workqueue: Vec<WorkRequest, U64>,
        busy: bool,
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
                ticktimer,
                workqueue: Vec::new(),
                busy: false,
                //tx_queue: Vec::new(),
                //rx_queue: Vec::new(),
                //in_progress: false,
            };

            xous::claim_interrupt(
                utra::com::COM_IRQ,
                handle_irq,
                (&mut xc) as *mut XousCom as *mut usize,
            )
            .expect("couldn't claim irq");
            xc
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

        pub fn process_queue(&mut self) {
            if !self.workqueue.is_empty() && !self.busy {
                self.busy = true;
                let work_descriptor = self.workqueue.swap_remove(0); // not quite FIFO, but Vec does not support FIFO (best we can do with "heapless")
                if work_descriptor.work.verb == ComState::STAT.verb {
                    let stats = self.get_battstats();
                    return_battstats(work_descriptor.sender, stats)
                        .expect("Could not return BattStatsNb value");
                } else {
                    error!(
                        "unimplemented work queue responder 0x{:x}",
                        work_descriptor.work.verb
                    );
                }
                self.busy = false;
            }
        }

        pub fn get_battstats(&mut self) -> BattStats {
            let mut stats = BattStats::default();

            self.txrx(ComState::GAS_GAUGE.verb);
            stats.current = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as i16;
            self.wait_txrx(ComState::LINK_READ.verb, Some(100)); // stby_current, not used here
            stats.voltage = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
            self.wait_txrx(ComState::LINK_READ.verb, Some(100)); // power register value, not used

            self.txrx(ComState::GG_SOC.verb);
            stats.soc = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
            self.txrx(ComState::GG_REMAINING.verb);
            stats.remaining_capacity = self.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));

            stats
        }
    }
}

// a stub to try to avoid breaking hosted mode for as long as possible.
#[cfg(not(target_os = "none"))]
mod implementation {
    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;
    use com_rs_ref as com_rs;
    use com_rs::*;
    use log::error;

    use heapless::Vec;
    use heapless::consts::*;

    pub struct XousCom {
        pub workqueue: Vec<WorkRequest, U64>,
        busy: bool,
    }

    impl XousCom {
        pub fn new() -> XousCom {
            XousCom {
                workqueue: Vec::new(),
                busy: false,
            }
        }

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

        pub fn process_queue(&mut self) {
            if !self.workqueue.is_empty() && !self.busy {
                self.busy = true;
                let work_descriptor = self.workqueue.swap_remove(0); // not quite FIFO, but Vec does not support FIFO (best we can do with "heapless")
                if work_descriptor.work.verb == ComState::STAT.verb {
                    let stats = self.get_battstats();
                    return_battstats(work_descriptor.sender, stats)
                        .expect("Could not return BattStatsNb value");
                } else {
                    error!(
                        "unimplemented work queue responder 0x{:x}",
                        work_descriptor.work.verb
                    );
                }
                self.busy = false;
            }
        }
    }
}

#[xous::xous_main]
fn xmain() -> ! {
    use crate::implementation::XousCom;
    use core::pin::Pin;
    use xous::buffer;
    use rkyv::archived_value_mut;

    log_server::init_wait().unwrap();
    info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    let com_sid = xns.register_name(api::SERVER_NAME_COM).expect("can't register server");
    info!("registered with NS -- {:?}", com_sid);

    // Create a new com object
    let mut com = XousCom::new();

    // create an array to track return connections for battery stats
    let mut battstats_conns: [Option<xous::CID>; 32] = [None; 32];
    // other future notification vectors shall go here

    info!("starting main loop");
    loop {
        let msg = xous::receive_message(com_sid).unwrap();
        trace!("Message: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
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
            Some(Opcode::BattStats) => {
                info!("batt stats request received");
                let stats = com.get_battstats();
                let raw_stats: [usize; 2] = stats.into();
                xous::return_scalar2(msg.sender, raw_stats[1], raw_stats[0])
                    .expect("couldn't return batt stats request");
                info!("done returning batt stats request");
            }
            Some(Opcode::BattStatsNb) => {
                for &maybe_conn in battstats_conns.iter() {
                    if let Some(conn) = maybe_conn {
                        com.workqueue
                            .push(WorkRequest {
                                work: ComState::STAT,
                                sender: conn,
                            })
                            .unwrap();
                    }
                }
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
            None => error!("unknown opcode"),
        }

        com.process_queue();
    }
}
