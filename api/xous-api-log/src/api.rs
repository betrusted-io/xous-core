#[repr(C, align(4096))]
pub struct LogRecord {
    pub file_length: u32,
    pub file: [u8; 128],
    pub line: Option<core::num::NonZeroU32>,
    pub module_length: u32,
    pub module: [u8; 128],
    pub level: u32,
    pub args_length: u32,
    pub args: [u8; 3820],
}

impl Default for LogRecord {
    fn default() -> Self {
        LogRecord {
            file_length: 0,
            file: [0u8; 128],
            line: None,
            module_length: 0,
            module: [0u8; 128],
            level: 0,
            args_length: 0,
            args: [0u8; 3820],
        }
    }
}

#[derive(Debug, PartialEq, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    /// A `LogRecord` message, delivering structured log output
    LogRecord = 0,

    /// A `&[u8]` destined for stdout
    StandardOutput = 1,

    /// A `&[u8]` destined for stderr
    StandardError = 2,

    /// A `xous::StringBuffer` containing this program's name
    ProgramName = 3,

    /// Try to log console output to a USB serial port. Best-effort only; failures will not crash, will not
    /// be noted
    TryHookUsbMirror = 4,
    UnhookUsbMirror = 5,

    /// A panic occurred, and a panic log is forthcoming
    PanicStarted = 1000,

    /// Log messages of varying size
    PanicMessage0 = 1100,
    PanicMessage1 = 1101,
    PanicMessage2 = 1102,
    PanicMessage3 = 1103,
    PanicMessage4 = 1104,
    PanicMessage5 = 1105,
    PanicMessage6 = 1106,
    PanicMessage7 = 1107,
    PanicMessage8 = 1108,
    PanicMessage9 = 1109,
    PanicMessage10 = 1110,
    PanicMessage11 = 1111,
    PanicMessage12 = 1112,
    PanicMessage13 = 1113,
    PanicMessage14 = 1114,
    PanicMessage15 = 1115,
    PanicMessage16 = 1116,
    PanicMessage17 = 1117,
    PanicMessage18 = 1118,
    PanicMessage19 = 1119,
    PanicMessage20 = 1120,
    PanicMessage21 = 1121,
    PanicMessage22 = 1122,
    PanicMessage23 = 1123,
    PanicMessage24 = 1124,
    PanicMessage25 = 1125,
    PanicMessage26 = 1126,
    PanicMessage27 = 1127,
    PanicMessage28 = 1128,
    PanicMessage29 = 1129,
    PanicMessage30 = 1130,
    PanicMessage31 = 1131,
    PanicMessage32 = 1132,

    /// End of a panic
    PanicFinished = 1200,

    /// Enable receiving messages when the system is resumed from sleep.
    EnableRx = 2000,
}
