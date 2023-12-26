// Note: the log server relies on this name not changing in order to hook the serial port for logging output.
// changing this name shouldn't lead to a crash, but it will lead to the USB driver being undiscoverable by the log crate.
pub(crate) const SERVER_NAME_USB_DEVICE: &'static str = "_Xous USB device driver_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Returns the link status
    LinkStatus = 0,
    /// Send a keyboard code
    SendKeyCode = 1,
    /// "Type" a string to the keyboard. This API is relied upon by the log crate.
    SendString = 2,
    /// Get the current LED state
    GetLedState = 3,
    /// Switch to a specified device core
    SwitchCores = 4,
    /// Makes sure a given core is selected
    EnsureCore = 5,
    /// Check which core is connected
    WhichCore = 6,
    /// Restrict the debug core
    RestrictDebugAccess = 7,
    /// Retrieve restriction state
    IsRestricted = 8,
    /// Set-and-check of USB debug restriction
    DebugUsbOp = 9,
    /// Set autotype rate
    SetAutotypeRate = 10,

    /// Send a U2F message
    U2fTx = 128,
    /// Blocks the caller, waiting for a U2F message
    U2fRxDeferred = 129,
    /// A bump from the timeout process to check if U2fRx has timed out
    U2fRxTimeout = 130,

    /// Query if the HID driver was able to start
    IsSocCompatible = 256,

    /// Hook serial ASCII listener
    SerialHookAscii = 512,
    /// Hook serial binary listener
    SerialHookBinary = 513,
    /// Flush any serial buffers
    SerialFlush = 514,
    /// Hook eager serial sender for TRNG output. This will not succeed if hooked for console mode already.
    SerialHookTrngSender = 515,
    /// Hook serial to the console input
    SerialHookConsole = 516,
    /// Clear any hooks
    SerialClearHooks = 517,
    /// TRNG send poll
    SerialTrngPoll = 518,

    #[cfg(feature="mass-storage")]
    SetBlockDevice = 1024,
    #[cfg(feature="mass-storage")]
    SetBlockDeviceSID = 1025,
    #[cfg(feature="mass-storage")]
    ResetBlockDevice = 1026,

    // HIDv2
    /// Read a HID report
    HIDReadReport = 1027,

    /// Write a HID report
    HIDWriteReport = 1028,

    /// Set the HID descriptor to be pushed to the USB host
    HIDSetDescriptor = 1029,

    /// Unset HID descriptor and reset HIDv2 state
    HIDUnsetDescriptor = 1030,

    /// Handle the USB interrupt
    UsbIrqHandler = 2048,
    /// Suspend/resume callback
    SuspendResume = 2049,
    /// Exits the server
    Quit = 4096,

    /// API used by the logging crate. The number is hard-coded; don't change it.
    LogString = 8192,
}

// The log crate depends on this API not changing.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UsbString {
    pub s: xous_ipc::String::<4000>,
    pub sent: Option<u32>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct U2fMsgIpc {
    /// All U2F protocol messages are 64 bytes
    pub data: [u8; 64],
    /// Encodes the state of the message
    pub code: U2fCode,
    /// Specifies an optional timeout
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq)]
pub enum U2fCode {
    Tx,
    TxAck,
    RxWait,
    RxAck,
    RxTimeout,
    Hangup,
    Denied,
}

#[derive(Eq, PartialEq, Copy, Clone)]
#[repr(usize)]
pub enum UsbDeviceType {
    Debug = 0,
    FidoKbd = 1,
    Fido = 2,
    #[cfg(feature="mass-storage")]
    MassStorage = 3,
    Serial = 4,
    HIDv2 = 5,
}
use std::convert::TryFrom;

impl TryFrom<usize> for UsbDeviceType {
    type Error = &'static str;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(UsbDeviceType::Debug),
            1 => Ok(UsbDeviceType::FidoKbd),
            2 => Ok(UsbDeviceType::Fido),
            #[cfg(feature="mass-storage")]
            3 => Ok(UsbDeviceType::MassStorage),
            4 => Ok(UsbDeviceType::Serial),
            5 => Ok(UsbDeviceType::HIDv2),
            _ => Err("Invalid UsbDeviceType specifier"),
        }
    }
}

pub const SERIAL_ASCII_BUFLEN: usize = 512;
pub const SERIAL_BINARY_BUFLEN: usize = 128;
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UsbSerialAscii {
    pub s: xous_ipc::String::<SERIAL_ASCII_BUFLEN>,
    pub delimiter: Option<char>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UsbSerialBinary {
    pub d: [u8; SERIAL_BINARY_BUFLEN],
    pub len: usize,
}

pub const MAX_HID_REPORT_DESCRIPTOR_LEN: usize = 1024;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct HIDReportDescriptorMessage {
    pub descriptor: [u8; MAX_HID_REPORT_DESCRIPTOR_LEN],
    pub len: usize,
}

#[derive(Copy, Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct HIDReport(pub [u8; 64]);

impl Default for HIDReport {
    fn default() -> Self {
        return Self([0u8; 64])
    }
}

#[derive(Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct HIDReportMessage {
    pub data: HIDReport,
    pub has_data: bool,
}