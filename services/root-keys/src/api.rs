pub(crate) const SERVER_NAME_KEYS: &str     = "_Root key server and update manager_";
#[allow(dead_code)]
pub(crate) const ROOTKEY_MODAL_NAME: &'static str = "rootkeys modal";
#[allow(dead_code)]
pub(crate) const ROOTKEY_MENU_NAME: &'static str = "rootkeys menu";

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// use to check if we've been initialized
    KeysInitialized,
    /// check that the digital signature on the gateware
    CheckGatewareSignature,

    TestUx,

    /// attempt to initialize keys on a brand new system. Does nothing if the keys are already provisioned.
    UxTryInitKeys,
    UxConfirmInitKeys,
    UxConfirmation,
    UxInitRequestPassword,
    UxInitPasswordReturn,
    UxGutter, // NOP for UX calls that require a destination
    UxGetPolicy,
    UxPolicyReturn,
    /// provision a gateware update with our secret data
    UxUpdateGateware,
    /// self-sign kernel/loader
    UxSelfSignXous,
    /// UX opcodes
    MenuRedraw,
    MenuKeys,
    MenuDrop,
    ModalRedraw,
    ModalKeys,
    ModalDrop,

    /// Suspend/resume callback
    SuspendResume,

    Quit
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum PasswordRetentionPolicy {
    AlwaysKeep,
    EraseOnSuspend,
    AlwaysPurge,
}

/// Enumerate the possible password types dealt with by this manager.
/// Note that the discriminant of the enum is used to every-so-slightly change the salt going into bcrypt
/// I don't think it hurts; more importantly, it also prevents an off-the-shelf "hashcat" run from
/// being used to brute force both passwords in a single go, as the salt has to be (slightly)
/// recomputed for each type of password.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PasswordType {
    Boot = 1,
    Update = 2,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RootkeyResult {
    AlignmentError,
    KeyError,
    IntegrityError,
    FlashError,
}
