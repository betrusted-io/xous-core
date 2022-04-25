use gam::modal::*;

pub(crate) const SERVER_NAME_MODALS: &str = "_Modal Dialog Server_";

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
    pub prompt: xous_ipc::String<1024>,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedListItem {
    pub token: [u32; 4],
    pub item: ItemName,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedPromptWithTextResponse {
    pub token: [u32; 4],
    pub prompt: xous_ipc::String<1024>,
    pub fields: u32,
    /// placeholders
    pub placeholders: Option<[Option<xous_ipc::String<256>>; 10]>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedNotification {
    pub token: [u32; 4],
    pub message: xous_ipc::String<1024>,
    pub qrtext: Option<xous_ipc::String<1024>>,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ManagedProgress {
    pub token: [u32; 4],
    pub title: xous_ipc::String<1024>,
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
    pub title: Option<xous_ipc::String<1024>>,
    pub text: Option<xous_ipc::String<2048>>,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    // these are blocking calls
    /// ask a question, get a single response from a list of defined items (radio box)
    PromptWithFixedResponse,
    /// ask a question, get multiple responses from a list of defined items (check box)
    PromptWithMultiResponse,
    /// simple notification
    Notification,
    /// dynamic notification - a simple non-interactive notification that allows its text to be dynamically updated
    DynamicNotification,

    /// ask a question, get a free-form answer back
    PromptWithTextResponse,
    /// must be used by the PromptWithTextResponse caller to acknowledge correct input
    TextResponseValid,

    // these are non-blocking calls
    /// add an item to the radio box or check box. Note that all added items
    /// are cleared after the relevant "action" call happens (PromptWith[Fixed,Multi]Response)
    AddModalItem,
    /// get the index of the selected radio button / checkboxes
    GetModalIndex,
    /// raise a progress bar
    StartProgress,
    /// update the progress bar
    DoUpdateProgress,
    /// lower a progress bar
    StopProgress,
    /// update a dynamic notification's text
    UpdateDynamicNotification,
    /// close dynamic notification
    CloseDynamicNotification,

    /// used by libraries to get the mutex on the server
    GetMutex,

    // these are used internally by the modals to handle intermediate state. Do not call from the outside.
    // these were originally handled in a separate thread for deferred responses using busy-waits. They are
    // now handled with deferred responses with makes code less complicated and less load on the CPU but
    // it does expose some of the internal API mechanics to outside processes. This is fine for the modals
    // box because it's a convenience routine used by "everyone"; password boxes are always handled within
    // a given secured server so that the attack surface for these do not extend into the modals boundary.
    InitiateOp,
    FinishProgress,

    TextEntryReturn,
    RadioReturn,
    CheckBoxReturn,
    NotificationReturn,

    DoUpdateDynamicNotification,
    DoCloseDynamicNotification,

    ModalRedraw,
    ModalKeypress,
    ModalDrop,
    Gutter,

    Quit,
}
