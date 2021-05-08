use crate::api::*;

pub(crate) struct I2cStateMachine {
}

impl I2cStateMachine {
    pub fn new(_handler_conn: xous::CID) -> Self {
        I2cStateMachine {
        }
    }
    pub fn suspend(&mut self) {}
    pub fn resume(&mut self) {}
    pub fn initiate(&mut self, _transaction: I2cTransaction) -> I2cStatus {
        I2cStatus::ResponseInProgress
    }
    pub fn report_write_done(&mut self) {
    }
    pub fn report_read_done(&mut self) {
    }
    pub fn is_busy(&self) -> bool {
        false
    }
}
