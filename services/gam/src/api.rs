use xous::{Message, ScalarMessage};
use graphics_server::api::{Point, Rectangle, TextView, TextViewResult, Gid, Line};
use blitstr_ref as blitstr;
use blitstr::{GlyphStyle, Cursor};

#[derive(Debug, rkyv::Archive, rkyv::Unarchive, Copy, Clone)]
// #[archive(derive(Copy, Clone))]
pub enum Opcode {
    // clears a canvas with a given GID
    ClearCanvas(Gid),

    // renders a TextView
    RenderTextView(TextView),

    // returns a GID to the "content" Canvas; requires an authentication token
    RequestContentCanvas(Gid),

    // hides a canvas with a given GID
    HideCanvas(Gid),

    // requests the GID to the "input" Canvas; call only works once (for the IME server), then maps out
    RequestInputCanvas,

    // indicates if the current UI layout requires an input field
    HasInput(bool),

    // return the dimensions of a canvas
    GetCanvasBounds(Gid),

    // draws a line on a canvas
    DrawLine(Gid, Line),
}

impl core::convert::TryFrom<&Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::ClearCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                _ => Err("GAM api: unknown Scalar ID"),
            },
            Message::BlockingScalar(m) => match m.id {
                0 => Ok(Opcode::GetCanvasBounds(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                _ => Err("GAM api: unknown BlockingScalar ID"),
            }
            _ => Err("GAM api: unhandled message type"),
        }
    }
}

impl Into<Message> for Opcode {
    fn into(self) -> Message {
        match self {
            Opcode::ClearCanvas(gid) => Message::Scalar(ScalarMessage {
                id: 0, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            Opcode::GetCanvasBounds(gid) => Message::BlockingScalar(ScalarMessage {
                id: 0, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            _ => panic!("GAM api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}
