use crate::api::*;
use std::sync::{Arc, atomic::AtomicBool};

pub(crate) struct I2cStateMachine {
}

impl I2cStateMachine {
    pub fn new(_handler_conn: xous::CID, _power_csr_raw: *mut u32, _wfi_state: Arc::<AtomicBool>) -> Self {
        I2cStateMachine {
        }
    }
    pub fn suspend(&mut self) {}
    pub fn resume(&mut self) {}
    pub fn re_initiate(&mut self) {}
    pub fn get_expiry(&self) -> Option::<u64> { None }
    pub fn initiate(&mut self, mut msg: xous::MessageEnvelope) {
        let mut buffer = unsafe { xous_ipc::Buffer::from_memory_message_mut(msg.body.memory_message_mut().unwrap()) };
        let transaction = buffer.to_original::<I2cTransaction, _>().unwrap();
        let response = if transaction.rxbuf.is_some() {
            I2cResult {
                rxbuf: [0u8; I2C_MAX_LEN],
                rxlen: transaction.rxbuf.unwrap().len() as u32,
                status: I2cStatus::ResponseReadOk,
            }
        } else {
            I2cResult {
                rxbuf: [0u8; I2C_MAX_LEN],
                rxlen: 0,
                status: I2cStatus::ResponseWriteOk,
            }
        };
        buffer.replace(response).unwrap();
    }
    pub fn report_write_done(&mut self) {
    }
    pub fn report_read_done(&mut self) {
    }
    pub fn is_busy(&self) -> bool {
        false
    }
    pub fn in_progress(&self) -> bool {
        false
    }
    pub fn trace(&self, _arg: usize) {
    }
    pub fn driver_reset(&mut self) {
    }
}
