use crate::api::*;

use utralib::*;

use num_traits::ToPrimitive;
use susres::{RegManager, RegOrField, SuspendResume};

#[derive(Eq, PartialEq, Debug)]
enum I2cState {
    Idle,
    Write,
    Read,
}
#[derive(Eq, PartialEq, Debug)]
enum I2cIntError {
    NoErr,
    NoTxn,
    MissingTx,
    MissingRx,
    UnexpectedState,
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
            I2cHandlerReport::InProgress => {
                if i2c.trace {
                    xous::try_send_message(conn,
                        xous::Message::new_scalar(I2cOpcode::IrqI2cTrace.to_usize().unwrap(), 0, 0, 0, 0)).map(|_| ()).unwrap();
                }
            },
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

    transaction: Option<I2cTransaction>,
    callback: Option<xous::MessageEnvelope>,
    expiry: Option<u64>, // timeout of any pending transaction

    state: I2cState,
    index: u32,  // index of the current buffer in the state machine
    ticktimer: ticktimer_server::Ticktimer, // a connection to the ticktimer so we can measure timeouts
    error: I2cIntError, // set if the interrupt handler encountered some kind of error
    trace: bool, // set to true for detailed tracing of I2C irq handler state behavior; note that the trace outputs are delayed and may not reflect actual status

    workqueue: Vec<(I2cTransaction, xous::MessageEnvelope)>,
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

            transaction: None,
            callback: None,

            state: I2cState::Idle,
            expiry: None,
            ticktimer,
            index: 0,
            error: I2cIntError::NoErr,
            trace: false,

            workqueue: Vec::new(),
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
    pub fn get_expiry(&self) -> Option<u64> {
        self.expiry
    }
    #[allow(dead_code)]
    pub fn set_trace(&mut self, trace: bool) {
        self.trace = trace;
    }
    pub fn suspend(&mut self) {
        self.i2c_susres.suspend();

        // this happens after suspend, so these disables are "lost" upon resume and replaced with the normal running values
        self.i2c_csr.wo(utra::i2c::EV_ENABLE, 0);
    }
    pub fn resume(&mut self) {
        self.i2c_susres.resume();
    }

    pub fn re_initiate(&mut self) {
        // execution continues after here because we simply drop the response message back in the sender's queue, and then return here to do more
        log::warn!("I2C timeout; resetting hardware block and re-trying");
        self.i2c_csr.wfo(utra::i2c::CORE_RESET_RESET, 1);
        self.ticktimer.sleep_ms(1).ok();

        // set the prescale assuming 100MHz cpu operation: 100MHz / ( 5 * 100kHz ) - 1 = 199
        let clkcode = (utralib::LITEX_CONFIG_CLOCK_FREQUENCY as u32) / (5 * 100_000) - 1;
        self.i2c_csr.wfo(utra::i2c::PRESCALE_PRESCALE, clkcode & 0xFFFF);
        // clear any interrupts pending
        self.i2c_csr.wo(utra::i2c::EV_PENDING, self.i2c_csr.r(utra::i2c::EV_PENDING));
        // enable the block
        self.i2c_csr.rmwf(utra::i2c::CONTROL_EN, 1);
        // cleanup state tracking
        self.state = I2cState::Idle;
        self.expiry = None;
        self.transaction = None;
        // re-initiate the I2C transaction
        if let Some(msg) = self.callback.take() {
            let transaction = {
                let buffer = unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                buffer.to_original::<I2cTransaction, _>().unwrap().clone()
            };
            self.checked_initiate(transaction, msg);
        }
    }

    pub fn initiate(&mut self, msg: xous::MessageEnvelope) {
        let transaction = {
            let buffer = unsafe { xous_ipc::Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
            buffer.to_original::<I2cTransaction, _>().unwrap().clone()
        };

        if let Some(expiry) = self.expiry {
            if (self.ticktimer.elapsed_ms() > expiry) || self.error != I2cIntError::NoErr {
                // previous transaction was in progress, and it timed out
                if self.error != I2cIntError::NoErr {
                    log::error!("I2C interrupt handler error: {:?}", self.error);
                    self.report_response(I2cStatus::ResponseInterruptError, None);
                } else {
                    self.report_response(I2cStatus::ResponseTimeout, None); // this resets all state variables back to defaults
                }
                // execution continues after here because we simply drop the response message back in the sender's queue, and then return here to do more
                log::warn!("I2C timeout; resetting hardware block");
                self.i2c_csr.wfo(utra::i2c::CORE_RESET_RESET, 1);
                // set the prescale assuming 100MHz cpu operation: 100MHz / ( 5 * 100kHz ) - 1 = 199
                let clkcode = (utralib::LITEX_CONFIG_CLOCK_FREQUENCY as u32) / (5 * 100_000) - 1;
                self.i2c_csr.wfo(utra::i2c::PRESCALE_PRESCALE, clkcode & 0xFFFF);
                // clear any interrupts pending
                self.i2c_csr.wo(utra::i2c::EV_PENDING, self.i2c_csr.r(utra::i2c::EV_PENDING));
                // enable the block
                self.i2c_csr.rmwf(utra::i2c::CONTROL_EN, 1);
            }
        }
        if self.callback.is_none() {
            assert!(self.state == I2cState::Idle, "previous call did not clean up correctly (state)");
            assert!(self.expiry.is_none(), "previous call did not clean up correctly (expiry)");
            assert!(self.transaction.is_none(), "previous call did not clean up correctly (transaction)");
            self.checked_initiate(transaction, msg);
        } else {
            log::debug!("I2C block is busy, pushing to work queue");
            self.workqueue.push((transaction, msg));
        }
    }

    /// Assumes we are initiating on a "clean" I2C machine (idle, no errors, no callbacks or state mapped)
    fn checked_initiate(&mut self, transaction: I2cTransaction, msg: xous::MessageEnvelope) {
        log::debug!("I2C initiated with {:x?}", transaction);
        // sanity-check the bounds limits
        if transaction.txlen > 258 || transaction.rxlen > 258 {
            self.report_response(I2cStatus::ResponseFormatError, None);
            return;
        }
        self.callback = Some(msg);
        self.expiry = Some(self.ticktimer.elapsed_ms() + transaction.timeout_ms as u64);

        // now do the BusAddr stuff, so that the we can get the irq response
        self.error = I2cIntError::NoErr;
        if transaction.txbuf.is_some() {
            // initiate bus address with write bit set
            self.state = I2cState::Write;
            self.i2c_csr.wfo(utra::i2c::TXR_TXR, (transaction.bus_addr << 1 | 0) as u32);
            self.transaction = Some(transaction);
            self.index = 0;
            self.i2c_csr.wo(utra::i2c::COMMAND,
                self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                self.i2c_csr.ms(utra::i2c::COMMAND_STA, 1)
            );
            log::debug!("Initiate write");
            self.trace();
        } else if transaction.rxbuf.is_some() {
            // initiate bus address with read bit set
            self.state = I2cState::Read;
            self.i2c_csr.wfo(utra::i2c::TXR_TXR, (transaction.bus_addr << 1 | 1) as u32);
            self.transaction = Some(transaction);
            self.index = 0;
            self.i2c_csr.wo(utra::i2c::COMMAND,
                self.i2c_csr.ms(utra::i2c::COMMAND_WR, 1) |
                self.i2c_csr.ms(utra::i2c::COMMAND_STA, 1)
            );
            log::debug!("Initiate read");
            self.trace();
        } else {
            // no buffers specified, erase everything and go to idle
            log::error!("Initiation error");
            self.trace();
            self.report_response(I2cStatus::ResponseFormatError, None);
            return;
        }
    }

    fn report_response(&mut self, status: I2cStatus, rx: Option<&[u8]>) {
        // the .take() will cause the msg to go out of scope, triggering Drop which unblocks the caller
        if let Some(mut msg) = self.callback.take() {
            let mut response = I2cResult {
                rxbuf: [0u8; I2C_MAX_LEN],
                rxlen: 0,
                status,
            };
            if let Some(data) = rx {
                for (&src, dst) in data.iter().zip(response.rxbuf.iter_mut()) {
                    *dst = src;
                }
                response.rxlen = data.len() as _;
            }
            let mut buf = unsafe {
                xous_ipc::Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap())
            };
            buf.replace(response).expect("couldn't serialize response to sender");
            log::debug!("transaction to None");
            self.transaction.take();
            self.expiry = None;
            self.state = I2cState::Idle;
            self.index = 0;
            self.error = I2cIntError::NoErr;
        } else {
            panic!("Invalid state: response requested but no request pending {:?}", status);
        }
        if self.workqueue.len() > 0 {
            log::debug!("workqueue has pending items: {}", self.workqueue.len());
            let (transaction, msg) = self.workqueue.remove(0);
            self.checked_initiate(transaction, msg);
        }
    }

    pub fn report_write_done(&mut self) {
        log::debug!("write_done");
        // report the end of a write-only transaction to all the listeners
        self.report_response(I2cStatus::ResponseWriteOk, None);
    }
    pub fn report_read_done(&mut self) {
        // report the result of a read transaction to all the listeners
        log::debug!("Sending read done {:?}", self.transaction);
        if let Some(transaction) = self.transaction {
            if let Some(rxbuf) = transaction.rxbuf {
                let mut rx = [0u8; I2C_MAX_LEN];
                for (&src, dst) in rxbuf[..transaction.rxlen as usize].iter().zip(rx.iter_mut()) {
                    *dst = src;
                }
                self.report_response(I2cStatus::ResponseReadOk, Some(&rx[..transaction.rxlen as usize]));
            } else {
                log::error!("Rx response but no buffer of data!");
                self.report_response(I2cStatus::ResponseFormatError, None);
            }
        } else {
            log::error!("Rx response but no transaction!");
            self.report_response(I2cStatus::ResponseFormatError, None);
        }
    }
    /// This will indicate the interface is busy if there is a transaction in progress or if there is
    /// work in the queue. The intention of this use case is if a caller is planning on doing a fairly
    /// extensive set of reads/writes sequentially and they want to volunarily back-off so they aren't overflowing
    /// the work queues or thrashing the bus by pulling it between two different peripherals.
    pub fn is_busy(&self) -> bool {
        if self.state == I2cState::Idle || self.workqueue.len() == 0 {
            false
        } else {
            true
        }
    }
    pub(crate) fn trace(&self) {
        log::debug!("I2C trace '{:?}/{:?}'=> PENDING: {:x}, ENABLE: {:x}, CMD: {:x}, STATUS: {:x}, CONTROL: {:x}, PRESCALE: {:x}",
            self.state,
            self.error,
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

        if let Some(transaction) = &mut self.transaction {
            match self.state {
                I2cState::Write => {
                    if let Some(txbuf) = transaction.txbuf {
                        // send next byte if there is one
                        if self.index < transaction.txlen {
                            self.i2c_csr.wfo(utra::i2c::TXR_TXR, txbuf[self.index as usize] as u32);
                            if self.index == (transaction.txlen - 1) && (transaction.rxbuf.is_none() || !transaction.use_repeated_start) {
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
                            if let Some(_rxbuf) = transaction.rxbuf {
                                // initiate bus address with read bit set
                                self.state = I2cState::Read;
                                self.i2c_csr.wfo(utra::i2c::TXR_TXR, (transaction.bus_addr << 1 | 1) as u32);
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
                        self.error = I2cIntError::MissingTx;
                    }
                },
                I2cState::Read => {
                    if let Some(rxbuf) = &mut transaction.rxbuf {
                        if self.index > 0 {
                            // we are re-entering from a previous call, store the read value from the previous call
                            rxbuf[self.index as usize - 1] = self.i2c_csr.rf(utra::i2c::RXR_RXR) as u8;
                        }
                        if self.index < transaction.rxlen {
                            if self.index == (transaction.rxlen - 1) {
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
                        self.error = I2cIntError::MissingRx;
                    }
                },
                I2cState::Idle => {
                    // this shouldn't happen, all we can do is flag an error
                    self.error = I2cIntError::UnexpectedState;
                }
            }
        } else {
            self.error = I2cIntError::NoTxn;
        }

        report
    }
}
