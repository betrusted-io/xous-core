use rkyv::{Archive, Deserialize, Serialize};

pub(crate) const SERVER_NAME_MTXCHAT: &str = "_Matrix chat_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MtxchatOp {
    /// chat ui event
    Event = 0,
    /// chat ui user post
    Post,
    /// chat ui keystroke
    Rawkeys,
    /// exit the application
    Quit,
}


