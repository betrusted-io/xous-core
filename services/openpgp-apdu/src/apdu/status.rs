//! ISO 7816-4 / OpenPGP card status words.

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum StatusWord {
    Success,
    MoreDataAvailable(u8),
    TerminationState,
    VerificationFailed,
    VerificationFailedRetries(u8),
    ExecutionError,
    MemoryFailure,
    WrongLength,
    SecurityStatusNotSatisfied,
    AuthMethodBlocked,
    ConditionsNotSatisfied,
    IncorrectParameters,
    FileNotFound,
    ReferenceDataNotFound,
    RecordNotFound,
    WrongParametersP1P2,
    InstructionNotSupported,
    ClassNotSupported,
    NoPreciseDiagnosis,
}

impl StatusWord {
    pub const SW_OK: u16 = 0x9000;

    pub fn sw1(self) -> u8 {
        match self {
            Self::Success => 0x90,
            Self::MoreDataAvailable(_) => 0x61,
            Self::TerminationState => 0x62,
            Self::VerificationFailed | Self::VerificationFailedRetries(_) => 0x63,
            Self::ExecutionError => 0x64,
            Self::MemoryFailure => 0x65,
            Self::WrongLength => 0x67,
            Self::SecurityStatusNotSatisfied
            | Self::AuthMethodBlocked
            | Self::ConditionsNotSatisfied => 0x69,
            Self::IncorrectParameters | Self::FileNotFound | Self::ReferenceDataNotFound | Self::RecordNotFound => {
                0x6A
            }
            Self::WrongParametersP1P2 => 0x6B,
            Self::InstructionNotSupported => 0x6D,
            Self::ClassNotSupported => 0x6E,
            Self::NoPreciseDiagnosis => 0x6F,
        }
    }

    pub fn sw2(self) -> u8 {
        match self {
            Self::Success => 0x00,
            Self::MoreDataAvailable(n) => n,
            Self::TerminationState => 0x85,
            Self::VerificationFailed => 0x00,
            Self::VerificationFailedRetries(x) => 0xC0 | (x & 0x0F),
            Self::ExecutionError => 0x00,
            Self::MemoryFailure => 0x81,
            Self::WrongLength => 0x00,
            Self::SecurityStatusNotSatisfied => 0x82,
            Self::AuthMethodBlocked => 0x83,
            Self::ConditionsNotSatisfied => 0x85,
            Self::IncorrectParameters => 0x80,
            Self::FileNotFound => 0x82,
            Self::ReferenceDataNotFound => 0x88,
            Self::RecordNotFound => 0x83,
            Self::WrongParametersP1P2 => 0x00,
            Self::InstructionNotSupported => 0x00,
            Self::ClassNotSupported => 0x00,
            Self::NoPreciseDiagnosis => 0x00,
        }
    }

    pub fn to_u16(self) -> u16 {
        u16::from(self.sw1()) << 8 | u16::from(self.sw2())
    }
}
