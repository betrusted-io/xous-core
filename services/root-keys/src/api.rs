pub(crate) const SERVER_NAME_KEYS: &str     = "_Root key server and update manager_";
pub(crate) const ROOTKEY_MODAL_NAME: &'static str = "rootkeys modal";


#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// attempt to initialize keys on a brand new system. Does nothing if the keys are already provisioned.
    TryInitKeysWithProgress,
    TryInitKeys,

    TestUx,

    /// Suspend/resume callback
    SuspendResume,

    Quit
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ProgressCallback {
    Update,
    Drop,
}

pub struct ProgressReport {
    pub current_step: u32,
    pub total_steps: u32,
    pub finished: bool,
}

pub(crate) enum PasswordRetentionPolicy {
    AlwaysKeep,
    EraseOnSuspend,
    EraseOnIdle,
    AlwaysPurge,
}
