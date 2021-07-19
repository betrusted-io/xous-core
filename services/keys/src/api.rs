pub(crate) const SERVER_NAME_KEYS: &str     = "_Root key server and update manager_";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {


    /// Suspend/resume callback
    SuspendResume,
}
