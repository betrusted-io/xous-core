pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// [Internal] com llio interrupt callback
    ComInterrupt,

    /// [Internal] run the network stack code
    NetPump,

    /// Suspend/resume callback
    SuspendResume,
}
