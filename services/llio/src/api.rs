/////////////////////// UART TYPE
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum UartType {
    Kernel,
    Log,
    Application,
    Invalid,
}
// from/to for Xous messages
impl From<usize> for UartType {
    fn from(code: usize) -> Self {
        match code {
            0 => UartType::Kernel,
            1 => UartType::Log,
            2 => UartType::Application,
            _ => UartType::Invalid,
        }
    }
}
impl Into<usize> for UartType {
    fn into(self) -> usize {
        match self {
            UartType::Kernel => 0,
            UartType::Log => 1,
            UartType::Application => 2,
            UartType::Invalid => 3,
        }
    }
}
// for the actual bitmask going to hardware
impl Into<u32> for UartType {
    fn into(self) -> u32 {
        match self {
            UartType::Kernel => 0,
            UartType::Log => 1,
            UartType::Application => 2,
            UartType::Invalid => 3,
        }
    }
}

/////////////////////// I2C
pub (crate) const I2C_MAX_LEN: usize = 33;
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
}
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum I2cCallback {
    Result,
    Drop,
}
// maybe once things stabilize, it's probably a good idea to make this structure private to the crate,
// and create a "public" version for return values via callbacks. But for now, it's pretty
// convenient to reach into the state of the I2C machine to debug problems in the callbacks.
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct I2cTransaction {
    pub bus_addr: u8,
    // write address and read address are encoded in the packet field below
    pub txbuf: Option<[u8; I2C_MAX_LEN]>, // long enough for a 256-byte operation + 2 bytes of "register address"
    pub txlen: u32,
    pub rxbuf: Option<[u8; I2C_MAX_LEN]>,
    pub rxlen: u32,
    pub listener: Option<(u32, u32, u32, u32)>, // where Rx split transactions should be routed to
    pub timeout_ms: u32,
    pub status: I2cStatus,
    pub callback_id: u32, // used by callback routines to help identify a return value
}
impl I2cTransaction {
    pub fn new() -> Self {
        I2cTransaction{ bus_addr: 0, txbuf: None, txlen: 0, rxbuf: None, rxlen: 0, timeout_ms: 500, status: I2cStatus::Uninitialized, listener: None, callback_id: 0 }
    }
}
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum I2cOpcode {
    /// initiate an I2C transaction
    I2cTxRx,
    /// from i2c interrupt handler (internal API only)
    IrqI2cTxrxWriteDone,
    IrqI2cTxrxReadDone,
    /// checks if the I2C engine is currently busy, for polling implementations
    I2cIsBusy,
    /// SuspendResume callback
    SuspendResume,
    Quit,
}

////////////////////////////////// VIBE
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VibePattern {
    Short,
    Long,
    Double,
}
impl From<usize> for VibePattern {
    fn from(pattern: usize) -> Self {
        match pattern {
            0 => VibePattern::Long,
            1 => VibePattern::Double,
            _ => VibePattern::Short,
        }
    }
}
impl Into<usize> for VibePattern {
    fn into(self) -> usize {
        match self {
            VibePattern::Long => 0,
            VibePattern::Double => 1,
            VibePattern::Short => 0xffff_ffff,
        }
    }
}

//////////////////////////////// CLOCK GATING (placeholder)
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum ClockMode {
    Low,
    AllOn,
}
impl From<usize> for ClockMode {
    fn from(mode: usize) -> Self {
        match mode {
            0 => ClockMode::Low,
            _ => ClockMode::AllOn,
        }
    }
}
impl Into<usize> for ClockMode {
    fn into(self) -> usize {
        match self {
            ClockMode::Low => 0,
            ClockMode::AllOn => 0xffff_ffff,
        }
    }
}

pub(crate) const SERVER_NAME_LLIO: &str      = "_Low Level I/O manager_";
pub(crate) const SERVER_NAME_I2C: &str       = "_Threaded I2C manager_";
//////////////////////////////////// OPCODES
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// not tested - set CRG parameters
    CrgMode, //(ClockMode),

    /// not tested -- set GPIO
    GpioDataOut, //(u32),
    GpioDataIn,
    GpioDataDrive, //(u32),
    GpioIntMask, //(u32),
    GpioIntAsFalling, //(u32),
    GpioIntPending,
    GpioIntEna, //(u32),
    GpioIntSubscribe, //(String<64>), // TODO
    GpioIntHappened,

    /// set UART mux
    UartMux, //(UartType),

    //TODO InfoLitexId, //(String<64>), // TODO: returns the ASCII string baked into the FPGA that describes the FPGA build, inside Registration
    InfoDna,
    InfoGit,
    InfoPlatform,
    InfoTarget,

    /// partially tested -- power
    PowerAudio, //(bool),
    PowerSelf, //(bool), // setting this to false allows the EC to turn off our power
    PowerBoostMode, //(bool),
    PowerCrypto,
    PowerCryptoStatus,
    WfiOverride,
    DebugPowerdown,
    DebugWakeup,
    EcSnoopAllow, //(bool),
    EcReset,
    EcPowerOn,
    SelfDestruct, //(u32), // requires a series of writes to enable

    /// vibe motor
    Vibe, //(VibePattern),

    /// not tested -- xadc
    AdcVbus,
    AdcVccInt,
    AdcVccAux,
    AdcVccBram,
    AdcUsbN,
    AdcUsbP,
    AdcTemperature,
    AdcGpio5,
    AdcGpio2,

    /// partially tested -- events
    EventComSubscribe, //(String<64>),
    EventRtcSubscribe, //(String<64>),
    EventUsbAttachSubscribe, //(String<64>),
    EventComEnable, //(bool),
    EventRtcEnable, //(bool),
    EventUsbAttachEnable, //(bool),

    /// internal from handler to main loop
    EventComHappened,
    EventRtcHappened,
    EventUsbHappened,

    /// SuspendResume callback
    SuspendResume,

    /// Exit the server
    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum EventCallback {
    Event,
    Drop,
}

/*
Implementation note: we use a ScalarHook with a two-stage message passing so we don't leak
the local SID. The local SID should be considered a secret, and not shared. A two-stage
message passing system creates a dedicated, one-time use server and shares this SID with
the LLIO server, thus protecting the local SID from disclosure.
*/
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32,  // ID of the scalar message to send through (e.g. the discriminant of the Enum on the caller's side API)
    pub cid: xous::CID,   // caller-side connection ID for the scalar message to route to. Created by the caller before hooking.
}

