use gam::modal::*;

pub(crate) const SERVER_NAME_MODALS: &str     = "_Modal Dialog Server_";

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
    pub token: [u32; 4],
    pub prompt: xous_ipc::String::<1024>,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedListItem {
    pub token: [u32; 4],
    pub item: ItemName,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedPromptWithTextResponse {
    pub token: [u32; 4],
    pub prompt: xous_ipc::String::<1024>,
    /// SID of a validator
    pub validator: Option<[u32; 4]>,
    /// the opcode to pass the validator
    pub validator_op: u32,
    /// the amount of fields to read
    pub fields: u32,
    /// placeholders
    pub placeholders: Option<[Option<xous_ipc::String::<256>>; 10]>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedNotification {
    pub token: [u32; 4],
    pub message: xous_ipc::String::<1024>,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedProgress {
    pub token: [u32; 4],
    pub title: xous_ipc::String::<1024>,
    // these are automatcally turned into percentages on a scale of 0->100%
    /// starting quanta to track (e.g. starting sector for erase).
    pub start_work: u32,
    /// end quanta to track (e.g. ending sector for erase). end is by definition the larger number than start.
    pub end_work: u32,
    /// current quanta of work. Used to int the bar, updates are just a scalar with the same value.
    pub current_work: u32,
}

/// This isn't a terribly useful notification -- it's basically read-only, no interactivity,
/// but you can animate the text. Mainly used for testing routines. Might be modifiable
/// into something more useful with a bit of thought, but for now, MVP.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct DynamicNotification {
    pub token: [u32; 4],
    pub title: Option<xous_ipc::String::<1024>>,
    pub text: Option<xous_ipc::String::<2048>>,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    // these are blocking calls
    /// ask a question, get a single response from a list of defined items (radio box)
    PromptWithFixedResponse,
    /// ask a question, get multiple responses from a list of defined items (check box)
    PromptWithMultiResponse,
    /// ask a question, get a free-form answer back
    PromptWithTextResponse,
    /// simple notification
    Notification,
    /// dynamic notification - a simple non-interactive notification that allows its text to be dynamically updated
    DynamicNotification,

    // these are non-blocking calls
    /// add an item to the radio box or check box. Note that all added items
    /// are cleared after the relevant "action" call happens (PromptWith[Fixed,Multi]Response)
    AddModalItem,
    /// raise a progress bar
    StartProgress,
    /// update the progress bar
    UpdateProgress,
    /// lower a progress bar
    StopProgress,
    /// update a dynamic notification's text
    UpdateDynamicNotification,
    /// close dynamic notification
    CloseDynamicNotification,

    GetMutex,

    Quit,
}
