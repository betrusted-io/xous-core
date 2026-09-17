#![allow(dead_code)]

/// GAM/xous-names server name for this app. Must be unique, < 64 chars.
pub(crate) const SERVER_NAME_MAIL: &str = "_Mail IMAP/SMTP client_";

/// F-key raw-key char codes, as delivered by the keyboard/GAM to a
/// `rawkeys`-registered app (same values the chat library decoded).
pub(crate) const F1: char = '\u{0011}';
pub(crate) const F2: char = '\u{0012}';
pub(crate) const F3: char = '\u{0013}';
pub(crate) const F4: char = '\u{0014}';

/// Opcodes the GAM dispatches to this app's server (the `*_id` fields of our
/// `UxRegistration`), plus Quit.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MailOp {
    /// GAM asks us to redraw our content canvas.
    Redraw = 0,
    /// A raw keystroke arrived (we decode F1..F4 from it).
    Rawkeys,
    /// A committed input line from the IME. Unused (we take no free-text
    /// input in the home screen), but registered so our UxRegistration
    /// mirrors the chat library's working shape (predictor + gotinput +
    /// rawkeys). Drained and ignored.
    Line,
    /// Focus gained/lost.
    ChangeFocus,
    /// Exit the application.
    Quit,
}
