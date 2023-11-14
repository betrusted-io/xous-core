#[allow(dead_code)]
pub(crate) const SERVER_NAME_SIGCHAT: &str = "_Signal chat_";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum SigchatOp {
    /// chat ui event
    Event = 0,
    /// menu item click
    Menu,
    /// chat ui user post
    Post,
    /// chat ui keystroke
    Rawkeys,
    /// exit the application
    Quit,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MenuOp {
    Link,
    Noop,
    Register,
}

#[allow(dead_code)]
pub struct Msg {
    pub type_: String,
    pub body: Option<String>,
    pub sender: Option<String>,
    pub ts: Option<u64>,
}
