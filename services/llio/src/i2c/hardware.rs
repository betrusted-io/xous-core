use crate::api::*;

use utralib::*;

use num_traits::ToPrimitive;
use susres::{RegManager, RegOrField, SuspendResume};
use heapless::spsc::Queue;

#[derive(Eq, PartialEq)]
enum I2cState {
    Idle,
    Write,
    Read,
}

// ASSUME: we are only ever handling txrx done interrupts. If implementing ARB interrupts, this needs to be refactored to read the source and dispatch accordingly.
fn handle_i2c_irq(_irq_no: usize, arg: *mut usize) {
    let i2c = unsafe { &mut *(arg as *mut I2cStateMachine) };

    if let Some(conn) = i2c.handler_conn {
        match i2c.handler_i() {
            I2cHandlerReport::WriteDone => {
                xous::try_send_message(conn,
                    xous::Message::new_scalar(I2cOpcode::IrqI2cTxrxWriteDone.to_usize().unwrap(), 0, 0, 0, 0)).map(|_| ()).unwrap();
            },
            I2cHandlerReport::ReadDone => {
                xous::try_send_message(conn,
                    xous::Message::new_scalar(I2cOpcode::IrqI2cTxrxReadDone.to_usize().unwrap(), 0, 0, 0, 0)).map(|_| ()).unwrap();
            },
            _ => {}, // don't send any message if we're in progress
        }
    } else {
        panic!("|handle_i2c_irq: TXRX done interrupt, but no connection for notification!");
    }
    i2c.i2c_csr
        .wo(utra::i2c::EV_PENDING, i2c.i2c_csr.r(utra::i2c::EV_PENDING));
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum I2cHandlerReport {
    WriteDone,
    ReadDone,
    InProgress,
}
pub(crate) struct I2cStateMachine {
    i2c_csr: utralib::CSR<u32>,
    i2c_susres: RegManager::<{utra::i2c::I2C_NUMREGS}>,
    handler_conn: Option<xous::CID>,

    transaction: I2cTransaction,
    state: I2cState,
    index: u32,  // index of the current buffer in the state machine
    timestamp: u64, // timestamp of the last transaction
    ticktimer: ticktimer_server::Ticktimer, // a connection to the ticktimer so we can measure timeouts
    listener: Option<xous::SID>,
    error: bool, // set if the interrupt handler encountered some kind of error

    workqueue: Queue<I2cTransaction, 8>,
}

impl I2cStateMachine {
    pub fn new(handler_conn: xous::CID) -> Self {
        let ticktimer = ticktimer_server::Ticktimer::new().expect("Couldn't connect to Ticktimer");
        let i2c_csr = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::i2c::HW_I2C_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map I2C CSR range");

        let mut i2c = I2cStateMachine {
            i2c_csr: CSR::new(i2c_csr.as_mut_ptr() as *mut u32),
            i2c_susres: RegManager::new(i2c_csr.as_mut_ptr() as *mut u32),
            handler_conn: Some(handler_conn),

            transaction: I2cTransaction::new(),
            state: I2cState::Idle,
            timestamp: ticktimer.elapsed_ms(),
            ticktimer,
            index: 0,
            listener: None,
            error: false,

            workqueue: Queue::new(),
        };

        // disable interrupt, just in case it's enabled from e.g. a warm boot
        i2c.i2c_csr.wfo(utra::i2c::EV_ENABLE_TXRX_DONE, 0);
        xous::claim_interrupt(
            utra::i2c::I2C_IRQ,
            handle_i2c_irq,
            (&mut i2c) as *mut I2cStateMachine as *mut usize,
        )
        .expect("couldn't claim I2C irq");

        // initialize i2c clocks
        // set the prescale assuming 100MHz cpu operation: 100MHz / ( 5 * 100kHz ) - 1 = 199
        let clkcode = (utralib::LITEX_CONFIG_CLOCK_FREQUENCY as u32) / (5 * 100_000) - 1;
        i2c.i2c_csr.wfo(utra::i2c::PRESCALE_PRESCALE, clkcode & 0xFFFF);
        // enable the block
        i2c.i2c_csr.rmwf(utra::i2c::CONTROL_EN, 1);
        // clear any interrupts pending, just in case something went pear-shaped during initialization
        i2c.i2c_csr.wo(utra::i2c::EV_PENDING, i2c.i2c_csr.r(utra::i2c::EV_PENDING));
        // now enable interrupts
        i2c.i2c_csr.wfo(utra::i2c::EV_ENABLE_TXRX_DONE, 1);

        // setup suspend/resume manager
        i2c.i2c_susres.push(RegOrField::Field(utra::i2c::PRESCALE_PRESCALE), None);
        i2c.i2c_susres.push(RegOrField::Reg(utra::i2c::CONTROL), None);
        i2c.i2c_susres.push_fixed_value(RegOrField::Reg(utra::i2c::EV_PENDING), 0xFFFF_FFFF); // clear pending interrupts
        i2c.i2c_susres.push(RegOrField::Reg(utra::i2c::EV_ENABLE), None);

        i2c
    }
    pub fn suspend(&mut self) {
        self.i2c_susres.suspend();

        // this happens after suspend, so these disables are "lost" upon resume and replaced with the normal running values
        self.i2c_csr.wo(utra::i2c::EV_ENABLE, 0);
    }
    pub fn resume(&mut self) {
        self.i2c_susres.resume();
    }

    pub fn initiate(&mut self, transaction: I2cTransaction ) -> I2cStatus {
        // state idle means the transaction is done
        // listener None means any callback notifications are also done
        // workqueue empty means, we have a clear path to do our thing
        if self.workqueue.is_empty() && self.state == I2cState::Idle && self.listener == None {
            self.checked_initiate(transaction)
        } else {
            match self.workqueue.enqueue(transaction) {
                Ok(_) => return I2cStatus::ResponseInProgress,
                _ => return I2cStatus::ResponseBusy,
            }
        }
    }

    fn checked_initiate(&mut self, transaction: I2cTransaction) -> I2cStatus {
        log::trace!("I2C initated with {:x?}", transaction);
        // sanity-check the bounds limits
        if transaction.txlen > 258 || transaction.rxlen > 258 {
            return I2cStatus::ResponseFormatError
        }

        let now = self.ticktimer.elapsed_ms();
        if self.state != I2cState::Idle && ((now - self.timestamp) < self.transaction.timeout_ms as u64) {
            // we're in a transaction that hadn't timed out, can't accept a new one
            log::trace!("bus is busy");
            I2cStatus::ResponseBusy
        } else {
            if self.state != I2cState::Idle {
                // we're in a transaction, but previous transaction had timed out...
                self.report_timeout();
                // reset our state parameter
                self.state = I2cState::Idle;
                self.index = 0;
                // reset the block - this resest just the state machine and not the prescaler or interrupt enable configs
                self.i2c_csr.wfo(utra::i2c::CORE_RESET_RESET, 1);
            }
            self.error = false;
            self.timestamp = now;
            self.transaction = transaction.clone();
            assert!(self.listener == None, "initiating when previous transaction is still in progress!");
            match transaction.listener {
                None => self.listener = None,
                Some((s0, s1, s2, s3)) => self.listener = Some(xous::SID::from_u32(s0, s1, s2, s3)),
            }
            log::trace!("initiate with listener {:?}", self.listener);

            if self.transaction.status == I2cStatus::RequestIncoming {
                self.transaction.status = I2cStatus::ResponseInProgress;
                // now do the BusAddr stuff, so that the we can get the irq response
                if let Some(_txbuf) = self.transaction.txbuf {
                    // initiate bus address with write bit set
                    self.state = I2cState::Write;
                    self.i2c_csr.wfo(utra::i2c::TXR_TXR, (self.transaction.bus_addr << 1 | 0) as u32);
                    self.index = 0;
                    self.i2c_csr.wo(utra::i2c::COMMAND,
                        self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                        self.i2c_csr.ms(utra::i2c::COMMAND_STA, 1)
                    );
                    self.trace();
                    I2cStatus::ResponseInProgress
                } else if let Some(_rxbuf) = self.transaction.rxbuf {
                    // initiate bus address with read bit set
                    self.state = I2cState::Read;
                    self.i2c_csr.wfo(utra::i2c::TXR_TXR, (self.transaction.bus_addr << 1 | 1) as u32);
                    self.index = 0;
                    self.i2c_csr.wo(utra::i2c::COMMAND,
                        self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                        self.i2c_csr.ms(utra::i2c::COMMAND_STA, 1)
                    );
                    self.trace();
                    I2cStatus::ResponseInProgress
                } else {
                    // no buffers specified, erase everything and go to idle
                    self.state = I2cState::Idle;
                    self.transaction = I2cTransaction::new();
                    self.trace();
                    I2cStatus::ResponseFormatError
                }
            } else {
                log::trace!("initiation format error");
                self.trace();
                I2cStatus::ResponseFormatError  // the status field was not formatted correctly to accept the transaction
            }
        }
    }

    fn i2c_followup(&mut self, trans: I2cTransaction) -> Result<(), xous::Error> {
        if let Some(listener) = self.listener.take() {
            log::trace!("followup to listener {:?}: {:?}", listener, trans);
            let cid = xous::connect(listener).unwrap();
            let buf = xous_ipc::Buffer::into_buf(trans).or(Err(xous::Error::InternalError))?;
            buf.lend(cid, I2cCallback::Result.to_u32().unwrap()).map(|_|())?;
            unsafe{xous::disconnect(cid).unwrap()};
        } else {
            log::trace!("completed with transaction, but no listener! {:?}", trans);
        };
        if let Some(work) = self.workqueue.dequeue() {
            if self.checked_initiate(work) != I2cStatus::ResponseInProgress {
                log::error!("Unable to initiate I2C transaction even though machine should be idle: {:?}.", work);
                log::error!("Probably, the I2C engine is going off the rails from here...");
            }
        };
        Ok(())
    }

    #[allow(dead_code)] // keep this around in case we figure out the NACK issue
    fn report_nack(&mut self) {
        log::trace!("NACK");
        // report the NACK situation to the listener
        let mut nack = I2cTransaction::new();
        nack.status = I2cStatus::ResponseNack;
        self.i2c_followup(nack).expect("couldn't send NACK to listeners");
    }
    fn report_timeout(&mut self) {
        log::error!("I2C timeout on transaction {:?}", self.transaction);
        let mut timeout = I2cTransaction::new();
        timeout.status = I2cStatus::ResponseTimeout;
        self.i2c_followup(timeout).expect("couldn't send timeout error to liseners");
    }
    pub fn report_write_done(&mut self) {
        log::trace!("write_done");
        // report the end of a write-only transaction to all the listeners
        let mut ack = I2cTransaction::new();
        ack.status = I2cStatus::ResponseWriteOk;
        self.i2c_followup(ack).expect("couldn't send write ACK to listeners");
    }
    pub fn report_read_done(&mut self) {
        log::trace!("read_done");
        // report the result of a read transaction to all the listeners
        self.transaction.status = I2cStatus::ResponseReadOk;
        log::trace!("Sending read done {:?}", self.transaction);
        self.i2c_followup(self.transaction).expect("couldn't send read response to listeners");
    }
    pub fn is_busy(&self) -> bool {
        if self.state == I2cState::Idle {
            false
        } else {
            true
        }
    }
    fn trace(&self) {
        log::trace!("I2C trace: PENDING: {:x}, ENABLE: {:x}, CMD: {:x}, STATUS: {:x}, CONTROL: {:x}, PRESCALE: {:x}",
            self.i2c_csr.r(utra::i2c::EV_PENDING),
            self.i2c_csr.r(utra::i2c::EV_ENABLE),
            self.i2c_csr.r(utra::i2c::COMMAND),
            self.i2c_csr.r(utra::i2c::STATUS),
            self.i2c_csr.r(utra::i2c::CONTROL),
            self.i2c_csr.r(utra::i2c::PRESCALE),
        );
    }

    // interrupt context-friendly handler
    pub(crate) fn handler_i(&mut self) -> I2cHandlerReport {
        let mut report = I2cHandlerReport::InProgress;

        match self.state {
            I2cState::Write => {
                if let Some(txbuf) = self.transaction.txbuf {
                    // send next byte if there is one
                    if self.index < self.transaction.txlen {
                        self.i2c_csr.wfo(utra::i2c::TXR_TXR, txbuf[self.index as usize] as u32);
                        if self.index == (self.transaction.txlen - 1) && self.transaction.rxbuf.is_none() {
                            // send a stop bit if this is the very last in the series
                            self.i2c_csr.wo(utra::i2c::COMMAND,
                                self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                                self.i2c_csr.ms(utra::i2c::COMMAND_STO, 1)
                            );
                        } else {
                            self.i2c_csr.wfo(utra::i2c::COMMAND_WR, 1);
                        }
                        self.index += 1;
                    } else {
                        if let Some(_rxbuf) = self.transaction.rxbuf {
                            // initiate bus address with read bit set
                            self.state = I2cState::Read;
                            self.i2c_csr.wfo(utra::i2c::TXR_TXR, (self.transaction.bus_addr << 1 | 1) as u32);
                            self.index = 0;
                            self.i2c_csr.wo(utra::i2c::COMMAND,
                                self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                                self.i2c_csr.ms(utra::i2c::COMMAND_STA, 1)
                            );
                        } else {
                            report = I2cHandlerReport::WriteDone;
                            self.state = I2cState::Idle;
                        }
                    }
                } else {
                    // we should never get here, because txbuf was checked as Some() by the setup routine
                }
            },
            I2cState::Read => {
                if let Some(mut rxbuf) = self.transaction.rxbuf {
                    if self.index > 0 {
                        // we are re-entering from a previous call, store the read value from the previous call
                        rxbuf[self.index as usize - 1] = self.i2c_csr.rf(utra::i2c::RXR_RXR) as u8;
                        self.transaction.rxbuf = Some(rxbuf);
                    }
                    if self.index < self.transaction.rxlen {
                        if self.index == (self.transaction.rxlen - 1) {
                            self.i2c_csr.wo(utra::i2c::COMMAND,
                                self.i2c_csr.ms(utra::i2c::COMMAND_RD, 1) |
                                self.i2c_csr.ms(utra::i2c::COMMAND_STO, 1) |
                                self.i2c_csr.ms(utra::i2c::COMMAND_ACK, 1)
                            );
                        } else {
                            self.i2c_csr.wfo(utra::i2c::COMMAND_RD, 1);
                        }
                        self.index += 1;
                    } else {
                        report = I2cHandlerReport::ReadDone;
                        self.state = I2cState::Idle;
                    }
                } else {
                    // we should never get here, because rxbuf was checked as Some() by the setup routine
                }
            },
            I2cState::Idle => {
                // this shouldn't happen, all we can do is flag an error
                self.error = true;
            }
        }

        report
    }
}
