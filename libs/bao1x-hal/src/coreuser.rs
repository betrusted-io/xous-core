use crate::protected_rram::{AccessSettings, AccessType};

#[repr(usize)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum CoreuserId {
    Boot0 = 1,
    Boot1 = 2,
    Fw0 = 4,
    Fw1 = 8,
}
impl Into<&'static str> for CoreuserId {
    fn into(self) -> &'static str {
        match self {
            Self::Boot0 => "Bt0",
            Self::Boot1 => "Bt1",
            Self::Fw0 => "Fw0",
            Self::Fw1 => "Fw1",
        }
    }
}
impl From<u32> for CoreuserId {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Boot0,
            2 => Self::Boot1,
            4 => Self::Fw0,
            8 => Self::Fw1,
            _ => unreachable!("invalid coreuser one-hot integer value"),
        }
    }
}

// what I want is a function that, given a DataSlotAccess, returns if access
// is allowable for a given CoreuserId
impl CoreuserId {
    pub fn is_accessible(&self, acl: &AccessSettings, access_type: &AccessType) -> bool {
        // these values are read out directly from the definition of DataSlotAccess
        let bitmask = (acl.raw_u32() >> 20) & 0xF;
        // the bitmask is stored as 0 == allowed. Invert the bits so that we can
        // do a simple AND bitmask against our own coding to determine if access is allowed.
        let as_allowed = bitmask ^ 0xF;

        // take advantage of the fact that our bit-coding of the enum matches
        // exactly the layout of the bit coding in the access control structure.
        let user_allowed = (*self as usize as u32 & as_allowed) != 0;

        // now compute if the requested access type is also allowed
        let access_allowed = match access_type {
            AccessType::None => true,
            AccessType::Read => acl.allows_cpu_read(),
            AccessType::Write => acl.allows_cpu_write(),
            AccessType::ReadWrite => acl.allows_cpu_read() && acl.allows_cpu_write(),
        };

        user_allowed & access_allowed
    }
}

/// This is the PID of the process that is designated to have trusted access to keys.
pub const TRUSTED_PID: u32 = 3;
