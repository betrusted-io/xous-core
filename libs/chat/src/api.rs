use enumset::EnumSetType;
use rkyv::{Archive, Deserialize, Serialize};



#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ChatOp {
    ///
    DialogueSet = 0,
    ///
    GamChangeFocus,
    /// a line of text has arrived
    GamLine,
    /// receive rawkeys from gam
    GamRawkeys,
    /// redraw our UI
    GamRedraw,
    ListenSet,
    MenuAdd,
    PostAdd,
    PostDel,
    PostFind,
    PostFlag,
    UiButton,
    UiMenu,
    /// exit the application
    Quit,
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
    pub author: xous_ipc::String<128>,
    pub timestamp: u64,
    pub text: xous_ipc::String<POST_TEXT_MAX>,
    pub attach_url: Option<xous_ipc::String<128>>,
}

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
