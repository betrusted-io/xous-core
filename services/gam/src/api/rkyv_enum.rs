// As of Rust 1.64.0:
//
// Rkyv-derived enums throw warnings that rkyv::Archive derived enums are never used
// and I can't figure out how to make them go away. Since they spam the build log,
// rkyv-derived enums are now isolated to their own file with a file-wide `dead_code`
// allow on top.
//
// This might be a temporary compiler regression, or it could just
// be yet another indicator that it's time to upgrade rkyv. However, we are waiting
// until rkyv hits 0.8 (the "shouldn't ever change again but still not sure enough
// for 1.0") release until we rework the entire system to chase the latest rkyv.
// As of now, the current version is 0.7.x and there isn't a timeline yet for 0.8.
#![allow(dead_code)]

use graphics_server::api::{Circle, Line, Rectangle, RoundedRectangle, TextView};

use crate::*;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum GamObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Return {
    UxToken(Option<[u32; 4]>),
    RenderReturn(TextView),
    SetCanvasBoundsReturn(SetCanvasBoundsRequest),
    ContentCanvasReturn(Option<Gid>),
    Failure,
    NotCurrentlyDrawable,
}

#[derive(Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone)]
pub(crate) enum MenuMgrOp {
    // incoming is one of these ops
    AddItem,
    InsertItem(usize),
    DeleteItem,
    SetIndex(usize),
    Quit,
    // response must be one of these
    Ok,
    Err,
}

#[allow(dead_code)] // here until Memory types are implemented
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum MenuPayload {
    /// memorized scalar payload
    Scalar([u32; 4]),
    /// this a nebulous-but-TBD maybe way of bodging in a more complicated record, which would involve
    /// casting this memorized, static payload into a Buffer and passing it on. Let's not worry too much
    /// about it for now, it's mostly apirational...
    Memory(([u8; 256], usize)),
}
