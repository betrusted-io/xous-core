#[cfg(feature = "ditherpunk")]
use gam::Tile;
#[cfg(not(any(feature = "hosted-baosec", feature = "cramium-soc")))]
use gam::modal::*;
#[cfg(any(feature = "hosted-baosec", feature = "cramium-soc"))]
use ux_api::widgets::*;

pub(crate) const SERVER_NAME_MODALS: &str = "_Modal Dialog Server_";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct Validation {
    pub text: TextEntryPayload,
    pub opcode: u32,
}
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ValidationOp {
    Validate,
    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedPromptWithFixedResponse {
    pub token: [u32; 4],
    pub prompt: String,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedListItem {
    pub token: [u32; 4],
    pub item: ItemName,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedPromptWithTextResponse {
    pub token: [u32; 4],
    pub prompt: String,
    pub fields: u32,
    /// placeholders
    pub placeholders: Option<[Option<(String, bool)>; 10]>,
    pub growable: bool,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedNotification {
    pub token: [u32; 4],
    pub message: String,
    // A Type 40 (177x177) qrcode with Medium data correction can encode max 3391 alphanumeric characters
    pub qrtext: Option<String>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedBip39 {
    pub token: [u32; 4],
    pub bip39_data: [u8; 32],
    pub bip39_len: u32,
    pub caption: Option<String>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
#[cfg(feature = "ditherpunk")]
pub struct ManagedImage {
    pub token: [u32; 4],
    pub tiles: [Option<Tile>; 6],
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct ManagedProgress {
    pub token: [u32; 4],
    pub title: String,
    // these are automatcally turned into percentages on a scale of 0->100%
    /// starting quanta to track (e.g. starting sector for erase).
    pub start_work: u32,
    /// end quanta to track (e.g. ending sector for erase). end is by definition the larger number than
    /// start.
    pub end_work: u32,
    /// current quanta of work. Used to int the bar, updates are just a scalar with the same value.
    pub current_work: u32,
    /// can user interact with it?
    pub user_interaction: bool,
    /// how much should the slider move with each movement?
    pub step: u32,
}

/// This isn't a terribly useful notification -- it's basically read-only, no interactivity,
/// but you can animate the text. Mainly used for testing routines. Might be modifiable
/// into something more useful with a bit of thought, but for now, MVP.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub struct DynamicNotification {
    pub token: [u32; 4],
    pub title: Option<String>,
    pub text: Option<String>,
}

/// API note: enums with explicit numbers may not have their numbers re-ordered, especially
/// not for aesthetic reasons! This is because when we assign numbers to enums, something else
/// is explicitly depending on that number in a way that will break if you change it (e.g.
/// FFI ABIs)
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    // these are blocking calls
    /// ask a question, get a single response from a list of defined items (radio box)
    PromptWithFixedResponse = 0,
    /// ask a question, get multiple responses from a list of defined items (check box)
    PromptWithMultiResponse = 1,
    /// simple notification
    Notification = 2,
    /// bip39 coded notification
    Bip39 = 31, // ---- note op number
    Bip39Input = 32,  // ----- note op number
    Bip39Return = 33, // ----- note op number
    SliderReturn = 34,
    Slider = 35,
    /// display an image
    #[cfg(feature = "ditherpunk")]
    Image = 3,
    /// dynamic notification - a simple non-interactive notification that allows its text to be dynamically
    /// updated
    DynamicNotification = 4,
    /// listen to dynamic notification - a blocking call, meant to be called from a separate thread from the
    /// control loop
    ListenToDynamicNotification = 5,

    /// ask a question, get a free-form answer back
    PromptWithTextResponse = 6,
    /// must be used by the PromptWithTextResponse caller to acknowledge correct input
    TextResponseValid = 7,

    // these are non-blocking calls
    /// add an item to the radio box or check box. Note that all added items
    /// are cleared after the relevant "action" call happens (PromptWith[Fixed,Multi]Response)
    AddModalItem = 8,
    /// get the index of the selected radio button / checkboxes
    GetModalIndex = 9,
    /// raise a progress bar
    StartProgress = 10,
    /// update the progress bar
    DoUpdateProgress = 11,
    /// lower a progress bar
    StopProgress = 12,
    /// update a dynamic notification's text
    UpdateDynamicNotification = 13,
    /// close dynamic notification
    CloseDynamicNotification = 14,

    /// used by libraries to get the mutex on the server
    GetMutex = 15,

    // these are used internally by the modals to handle intermediate state. Do not call from the outside.
    // these were originally handled in a separate thread for deferred responses using busy-waits. They are
    // now handled with deferred responses with makes code less complicated and less load on the CPU but
    // it does expose some of the internal API mechanics to outside processes. This is fine for the modals
    // box because it's a convenience routine used by "everyone"; password boxes are always handled within
    // a given secured server so that the attack surface for these do not extend into the modals boundary.
    InitiateOp = 16,
    FinishProgress = 17,

    TextEntryReturn = 18,
    RadioReturn = 19,
    CheckBoxReturn = 20,
    NotificationReturn = 21,
    #[cfg(feature = "ditherpunk")]
    ImageReturn = 22,

    DoUpdateDynamicNotification = 23,
    DoCloseDynamicNotification = 24,
    HandleDynamicNotificationKeyhit = 25,

    ModalRedraw = 26,
    ModalKeypress = 27,
    ModalDrop = 28,
    Gutter = 29,

    Quit = 30,
}
