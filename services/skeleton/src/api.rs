pub(crate) const SERVER_NAME_CODEC: &str     = "_Any descriptive and unique name under 64 chars_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Suspend/resume callback
    SuspendResume,
    /// Exits the server
    Quit,
}
