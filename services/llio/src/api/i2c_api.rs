use rkyv::{Archive, Deserialize, Serialize};

// ///////////////////// I2C
pub(crate) const SERVER_NAME_I2C: &str = "_Threaded I2C manager_";
// a small book-keeping struct used to report back to I2C requestors as to the status of a transaction
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Eq, PartialEq)]
pub enum I2cStatus {
    /// used only as the default, should always be set to one of the below before sending
    Uninitialized,
    /// used by a managing process to indicate a request
    RequestIncoming,
    /// everything was OK, request in progress
    ResponseInProgress,
    /// we tried to process your request, but there was a timeout
    ResponseTimeout,
    /// I2C had a NACK on the request
    ResponseNack,
    /// the I2C bus is currently busy and your request was ignored
    ResponseBusy,
    /// the request was malformed
    ResponseFormatError,
    /// everything is OK, data here should be valid
    ResponseReadOk,
    /// everything is OK, write finished. data fields have no meaning
    ResponseWriteOk,
    /// interrupt handler error
    ResponseInterruptError,
}
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum I2cCallback {
    Result,
    Drop,
}
// maybe once things stabilize, it's probably a good idea to make this structure private to the crate,
// and create a "public" version for return values via callbacks. But for now, it's pretty
// convenient to reach into the state of the I2C machine to debug problems in the callbacks.
#[allow(dead_code)]
pub(crate) const I2C_MAX_LEN: usize = 33;
#[derive(Debug, Copy, Clone, Archive, Serialize, Deserialize)]
pub struct I2cTransaction {
    pub bus_addr: u8,
    // write address and read address are encoded in the packet field below
    pub txbuf: Option<[u8; I2C_MAX_LEN]>,
    pub txlen: u32,
    pub rxbuf: Option<[u8; I2C_MAX_LEN]>,
    pub rxlen: u32,
    pub timeout_ms: u32,
    pub use_repeated_start: bool,
}
impl I2cTransaction {
    pub fn new() -> Self {
        I2cTransaction {
            bus_addr: 0,
            txbuf: None,
            txlen: 0,
            rxbuf: None,
            rxlen: 0,
            timeout_ms: 500,
            use_repeated_start: true,
        }
    }
}
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum I2cOpcode {
    /// initiate an I2C transaction
    I2cTxRx,
    /// from i2c interrupt handler (internal API only)
    IrqI2cTxrxWriteDone,
    IrqI2cTxrxReadDone,
    IrqI2cTrace,
    /// checks if the I2C engine is currently busy, for polling implementations
    I2cIsBusy,
    /// grabs a mutex on the I2C block, for multiple transactions that can't be separated
    I2cMutexAcquire,
    I2cMutexRelease,
    /// timeout check
    I2cTimeout,
    /// block soft reset
    I2cDriverReset,
    /// SuspendResume callback
    SuspendResume,
    Quit,
}

/// The data reported by an I2cAsycReadHook message
#[derive(Debug, Copy, Clone, Archive, Serialize, Deserialize)]
pub struct I2cResult {
    pub rxbuf: [u8; I2C_MAX_LEN],
    pub rxlen: u32,
    pub status: I2cStatus,
}
