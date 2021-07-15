#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get the elapsed time in milliseconds
    ElapsedMs = 0,

    /// Sleep for the specified numer of milliseconds
    SleepMs = 1,

    /// Recalculate the sleep time
    RecalculateSleep = 2,

    /// Suspend/resume callback
    SuspendResume = 3,

    /// force a WDT update
    PingWdt = 4,
}
