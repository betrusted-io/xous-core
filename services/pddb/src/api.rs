pub(crate) const SERVER_NAME_PDDB: &str     = "_Plausibly Deniable Database_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {

    /// Suspend/resume callback
    SuspendResume,
}
