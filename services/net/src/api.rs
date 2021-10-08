pub(crate) const SERVER_NAME_NET: &str     = "_Middleware Network Server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// com llio interrupt callback
    ComInterrupt,

    /// Suspend/resume callback
    SuspendResume,
}
