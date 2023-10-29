use enumset::EnumSetType;
use rkyv::{Archive, Deserialize, Serialize};

// shorthand for the function keys F1 - F4
pub const F1: char = '\u{0011}';
pub const F2: char = '\u{0012}';
pub const F3: char = '\u{0013}';
pub const F4: char = '\u{0014}';

// these are used to increment and decrement the selected post
pub const POST_SELECTED_NEXT: usize = usize::MAX - 0;
pub const POST_SELECTED_PREV: usize = usize::MAX - 1;

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ChatOp {
    // Save the Dialogue to pddb (ie after PostAdd, PostDelete)
    DialogueSave = 0,
    /// Set the current Dialogue to be displayed
    DialogueSet,
    /// change the Chat UI in/out of focus
    GamChangeFocus,
    /// a line of text has arrived
    GamLine,
    /// receive rawkeys from gam
    GamRawkeys,
    /// redraw our Chat UI
    GamRedraw,
    /// Show some user help
    Help,
    /// Add a new MenuItem to the App menu
    MenuAdd,
    /// Add a new Post to the Dialogue
    PostAdd,
    /// Delete a Post from the Dialogue
    PostDel,
    /// Find a Post by timestamp and Author
    PostFind,
    PostFlag,
    /// Set status bar text
    SetStatusText,
    /// Run or stop the busy animation.
    SetBusyAnimationState,
    /// Set the status idle text (to be shown when exiting all busy states)
    SetStatusIdleText,
    /// Update just the state of the busy animation, if any. Internal opcode.
    /// Will skip the update if called too often.
    UpdateBusy,
    /// Force update the busy bar, without rate throttling. Internal opcode.
    UpdateBusyForced,
    /// exit the application
    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum BusyAnimOp {
    Start,
    Pump,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum IconOp {
    PostMenu = 0,
    F2Op,
    F3Op,
    AppMenu,
}

pub const POST_TEXT_MAX: usize = 3072;

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Find {
    pub author: xous_ipc::String<128>,
    pub timestamp: u64,
    pub key: Option<usize>, // the return post key if found.
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Dialogue {
    pub dict: xous_ipc::String<128>,
    pub key: Option<xous_ipc::String<128>>,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct Post {
    pub dialogue_id: xous_ipc::String<128>,
    pub author: xous_ipc::String<128>,
    pub timestamp: u64,
    pub text: xous_ipc::String<POST_TEXT_MAX>,
    pub attach_url: Option<xous_ipc::String<128>>,
}

/// Events are sent to the Chat App when key things occur in the Chat UI
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Event {
    Focus,
    F1,     // F1 button click
    F2,     // F2 button click
    F3,     // you get the idea
    F4,     // guess
    Up,     // Up click
    Down,   // Down click
    Left,   // Left click
    Right,  // Right click
    Top,    // Top of post list reached
    Bottom, // Bottom of post list reached
    Key,    // keystroke
    Post,   // new user Post committed
    Menu,   // menu item clicked
}

#[derive(
    Archive,
    Serialize,
    Deserialize,
    Debug,
    num_derive::FromPrimitive,
    num_derive::ToPrimitive,
    EnumSetType,
)]
pub enum PostFlag {
    Deleted,
    Draft,
    Hidden,
}

#[derive(
    Archive,
    Serialize,
    Deserialize,
    Debug,
    num_derive::FromPrimitive,
    num_derive::ToPrimitive,
    EnumSetType,
)]
pub enum AuthorFlag {
    Bold,
    Hidden,
    Right,
}

#[derive(Archive, Serialize, Deserialize, Debug)]
pub struct BusyMessage {
    pub busy_msg: xous_ipc::String<128>,
}
