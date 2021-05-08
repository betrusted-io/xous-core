pub(crate) const SERVER_NAME_CODEC: &str     = "_Low-level Audio Codec Server_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    

    /// Suspend/resume callback
    SuspendResume,
}
