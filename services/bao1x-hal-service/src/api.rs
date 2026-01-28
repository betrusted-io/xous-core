/// This is a "well known name" used by `libstd` to connect to the time server
/// Anyone who wants to check if time has been initialized would use this name.
pub const TIME_SERVER_PUBLIC: &'static [u8; 16] = b"timeserverpublic";

/// Do not modify the discriminants in this structure. They are used in `libstd` directly.
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum TimeOp {
    /// Sync offsets to hardware RTC
    HwSync = 0,
    SetUtcTimeMs = 2,
    /// Get UTC time in ms since EPOCH
    GetUtcTimeMs = 3, // this is the one API call that `std` relies upon
    /// Get local time in ms since EPOCH
    GetLocalTimeMs = 4,
    /// Sets the timezone offset, in milliseconds.
    SetTzOffsetMs = 5,
    /// Query to see if timezone and time relative to UTC have been set.
    WallClockTimeInit = 6,

    /// Serialize the internal state for storage across reboots
    GetState = 1024,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ClockOp {
    GetVco,
    GetFclk,
    GetAclk,
    GetHclk,
    GetIclk,
    GetPclk,
    GetPer,
    DeepSleep,
}
