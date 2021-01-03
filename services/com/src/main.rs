#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

mod api;
use api::Opcode;

use core::convert::TryFrom;

use log::{error, info};

use com_rs::*;

use xous::CID;

const STD_TIMEOUT: u32 = 100;
#[derive(Debug, Copy, Clone)]
pub struct WorkRequest {
    work: ComSpec,
    sender: CID,
}

fn return_battstats(cid: CID, stats: api::BattStats) -> Result<(), xous::Error> {
    xous::send_message(cid, crate::api::Opcode::BattStatsReturn(stats).into()).map(|_| ())
}

#[cfg(target_os = "none")]
mod implementation {
    use crate::api::BattStats;
    use crate::return_battstats;
    use crate::WorkRequest;
    use com_rs::*;
    use log::{error, info};
    use ticktimer_server::*;
    use utralib::generated::*;
    use xous::CID;

    #[macro_use]
    use heapless::Vec;
    use heapless::consts::*;

    /*
        use typenum::{UInt, UTerm};
        use typenum::bit::{B0, B1};
        type U1280 = UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UInt<UTerm, B1>, B0>, B1>, B0>, B0>, B0>, B0>, B0>, B0>, B0>, B0>;
    */
    const STD_TIMEOUT: u32 = 100;

    pub struct XousCom {
        csr: utralib::CSR<u32>,
        ticktimer: CID,
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

            let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
            let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

            let mut xc = XousCom {
                csr: CSR::new(csr.as_mut_ptr() as *mut u32),
                ticktimer: ticktimer_conn,
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
                let curtime = ticktimer_server::elapsed_ms(self.ticktimer)
                    .expect("couldn't connect to ticktimer");
                let mut timed_out = false;
                let to = timeout.unwrap() as u64;
                while self.csr.rf(utra::com::STATUS_HOLD) == 1 && !timed_out {
                    if (ticktimer_server::elapsed_ms(self.ticktimer)
                        .expect("couldn't connect to ticktimer")
                        - curtime)
                        > to
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
    use com_rs::*;
    use log::{error, info};

    #[macro_use]
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

    log_server::init_wait().unwrap();

    let com_server =
        xous::create_server_with_address(b"com             ").expect("Couldn't create COM server");

    let shell_id = xous::SID::from_bytes(b"shell           ").unwrap();
    let shell_conn = xous::connect(shell_id).unwrap();

    let agent_conn: usize;
    if cfg!(feature = "fccagent") {
        let agent_id = xous::SID::from_bytes(b"fcc-agent-server").unwrap();
        agent_conn = xous::connect(agent_id).expect("Couldn't connect to fcc-agent! Are you building with the right features enabled?");
    } else {
        agent_conn = 0; // bogus value
    }

    // Create a new com object
    let mut com = XousCom::new();

    loop {
        info!("COM: waiting for message");
        let envelope = xous::receive_message(com_server).unwrap();
        info!("COM: Message: {:?}", envelope);
        if let Ok(opcode) = Opcode::try_from(&envelope.body) {
            info!("COM: Opcode: {:?}", opcode);
            match opcode {
                Opcode::PowerOffSoc => {
                    info!("COM: power off called");
                    com.txrx(ComState::POWER_OFF.verb);
                }
                Opcode::BattStats => {
                    info!("COM: batt stats request received");
                    let stats = com.get_battstats();
                    let raw_stats: [usize; 2] = stats.into();
                    xous::return_scalar2(envelope.sender, raw_stats[1], raw_stats[0])
                        .expect("COM: couldn't return batt stats request");
                    info!("COM: done returning batt stats request");
                }
                Opcode::BattStatsNb => {
                    com.workqueue
                        .push(WorkRequest {
                            work: ComState::STAT,
                            sender: shell_conn,
                        })
                        .unwrap();
                }
                Opcode::Wf200Rev => {
                    com.txrx(ComState::WFX_FW_REV_GET.verb);
                    let major = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                    let minor = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                    let build = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                    xous::return_scalar(
                        envelope.sender,
                        ((major as usize) << 16) | ((minor as usize) << 8) | (build as usize)
                    )
                    .expect("COM: couldn't return WF200 firmware rev");
                }
                Opcode::EcGitRev => {
                    com.txrx(ComState::EC_GIT_REV.verb);
                    let rev_msb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                    let rev_lsb = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u16;
                    let dirty = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT)) as u8;
                    xous::return_scalar2(
                        envelope.sender,
                        ((rev_msb as usize) << 16) | (rev_lsb as usize),
                        dirty as usize
                    )
                    .expect("COM: couldn't return WF200 firmware rev");
                }
                Opcode::Wf200PdsLine(l) => {
                    info!("COM: Wf200PdsLine got line {}", l);
                    let line = l.as_bytes();
                    let length = line.len() as u16;
                    //info!("COM: 0x{:04x}", ComState::WFX_PDS_LINE_SET.verb);
                    com.txrx(ComState::WFX_PDS_LINE_SET.verb);
                    //info!("COM: 0x{:04x}", length);
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
                        //info!("COM: 0x{:04x}", word);
                    }
                }
                Opcode::RxStatsAgent => {
                    xous::send_message(agent_conn, xous::Message::Scalar(
                        xous::ScalarMessage { id: 1, arg1: '!' as usize, arg2: 0, arg3: 0, arg4: 0}
                    ));
                    let mut stats: [u8; (ComState::WFX_RXSTAT_GET.r_words*2) as usize] = [0; (ComState::WFX_RXSTAT_GET.r_words*2) as usize];
                    /*
                    com.txrx(ComState::WFX_RXSTAT_GET.verb);
                    for i in 0..ComState::WFX_RXSTAT_GET.r_words as usize {
                        let data = com.wait_txrx(ComState::LINK_READ.verb, Some(STD_TIMEOUT));
                        stats[i*2] = data as u8;
                        stats[i*2+1] = (data >> 8) as u8;
                    }*/
                    if cfg!(feature = "fccagent") {
                        // hard-coded from fccagent to break circular dependency of fcc agent on com on agent on com on...
                        let data = xous::carton::Carton::from_bytes(&stats);
                        let m = xous::Message::Borrow(data.into_message(2));
                        xous::send_message(agent_conn, m);
                    }
                    xous::send_message(agent_conn, xous::Message::Scalar(
                        xous::ScalarMessage { id: 1, arg1: '@' as usize, arg2: 0, arg3: 0, arg4: 0}
                    ));
                }
                _ => error!("unknown opcode"),
            }
        } else {
            error!("couldn't convert opcode");
        }

        com.process_queue();
    }
}
