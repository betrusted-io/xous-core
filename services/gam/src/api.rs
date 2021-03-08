use xous::{Message, ScalarMessage};
use graphics_server::api::{Rectangle, TextView, Gid, Line, RoundedRectangle, Circle, Point};

#[derive(Debug, rkyv::Archive, rkyv::Unarchive, Copy, Clone)]
pub enum GamObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
}

#[derive(Debug, rkyv::Archive, rkyv::Unarchive, Copy, Clone)]
pub struct GamObject {
    pub canvas: Gid,
    pub obj: GamObjectType,
}

#[derive(Debug, rkyv::Archive, rkyv::Unarchive, Copy, Clone)]
pub struct SetCanvasBoundsRequest {
    pub canvas: Gid,
    pub requested: Point,
    pub granted: Option<Point>,
}

#[derive(Debug, rkyv::Archive, rkyv::Unarchive, Copy, Clone)]
// #[archive(derive(Copy, Clone))]
pub enum Opcode {
    // clears a canvas with a given GID
    ClearCanvas(Gid),

    // return the dimensions of a canvas as a Point (the top left is always (0,0))
    GetCanvasBounds(Gid),

    // request a new size for my canvas.
    // This normally will be denied, unless the requested Gid corresponds to a special canvas that allows resizing.
    SetCanvasBounds(SetCanvasBoundsRequest),

    // draws an object
    RenderObject(GamObject),

    // renders a TextView
    RenderTextView(TextView),

    // forces a redraw (which also does defacement, etc.)
    Redraw,

    /////// planned

    // returns a GID to the "content" Canvas; requires an authentication token
    RequestContentCanvas(Gid),

    // hides a canvas with a given GID
    HideCanvas(Gid),

    // requests the GID to the "input" Canvas; call only works once (for the IME server), then maps out
    RequestInputCanvas,

    // indicates if the current UI layout requires an input field
    HasInput(bool),

}

impl core::convert::TryFrom<&Message> for Opcode {
    type Error = &'static str;
    fn try_from(message: &Message) -> Result<Self, Self::Error> {
        match message {
            Message::Scalar(m) => match m.id {
                0 => Ok(Opcode::ClearCanvas(Gid::new([m.arg1 as _, m.arg2 as _, m.arg3 as _, m.arg4 as _]))),
                1 => Ok(Opcode::Redraw),
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
            Opcode::Redraw => Message::Scalar(ScalarMessage {
                id: 1, arg1: 0, arg2: 0, arg3: 0, arg4: 0
            }),
            Opcode::GetCanvasBounds(gid) => Message::BlockingScalar(ScalarMessage {
                id: 0, arg1: gid.gid()[0] as _, arg2: gid.gid()[1] as _, arg3: gid.gid()[2] as _, arg4: gid.gid()[3] as _
            }),
            _ => panic!("GAM api: Opcode type not handled by Into(), maybe you meant to use a helper method?"),
        }
    }
}
