use gam::modal::*;

pub(crate) const SERVER_NAME_MODALS: &str     = "_Modal Dialog Server_";
pub const SHARED_MODAL_NAME: &str = "shared modal";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct Validation {
    pub text: TextEntryPayload,
    pub opcode: u32,
}
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ValidationOp {
    Validate,
    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedPromptWithFixedResponse {
    pub prompt: xous_ipc::String::<1024>,
    pub items: [Option<ItemName>; MAX_ITEMS],
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedPromptWithTextResponse {
    pub prompt: xous_ipc::String::<1024>,
    /// SID of a validator
    pub validator: Option<[u32; 4]>,
    /// the opcode to pass the validator
    pub validator_op: u32,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedNotification {
    pub message: xous_ipc::String::<1024>,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedProgress {
    pub title: xous_ipc::String::<1024>,
    // these are automatcally turned into percentages on a scale of 0->100%
    /// starting quanta to track (e.g. starting sector for erase).
    pub start_work: u32,
    /// end quanta to track (e.g. ending sector for erase). end is by definition the larger number than start.
    pub end_work: u32,
    /// current quanta of work. Used to int the bar, updates are just a scalar with the same value.
    pub current_work: u32,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    // these are blocking calls
    /// ask a question, get a single response from a list of defined items
    PromptWithFixedResponse,
    /// ask a question, get multiple responses from a list of defined items
    PromptWithMultiResponse,
    /// ask a question, get a free-form answer back
    PromptWithTextResponse,
    /// simple notification
    Notification,

    // these are non-blocking calls
    /// raise a progress bar
    StartProgress,
    /// update the progress bar
    UpdateProgress,
    /// lower a progress bar
    StopProgress,
    Quit,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum UxOpcode {
    // add UX opcodes here, separate from the main loop's
    Format,
    OkCancelNotice,
    OkNotice,
    UnlockBasis,
    LockBasis,
    LockAllBasis,
    Scuttle,

    PasswordReturn,
    ModalRedraw,
    ModalKeys,
    ModalDrop,
    Gutter,
    Quit,
}
