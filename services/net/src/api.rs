pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// com llio interrupt callback
    ComInterrupt,

    /// run the network stack code
    NetPump,

    /// Suspend/resume callback
    SuspendResume,
}
