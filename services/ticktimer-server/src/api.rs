#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get the elapsed time in milliseconds
    ElapsedMs,

    /// Sleep for the specified numer of milliseconds
    SleepMs,

    /// Recalculate the sleep time
    RecalculateSleep,
}
