use graphics_server::api::{Rectangle, TextView, Gid, Line, RoundedRectangle, Circle, Point};
use xous_ipc::String;

pub(crate) const SERVER_NAME_GAM: &str      = "_Graphical Abstraction Manager_";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum GamObjectType {
    Line(Line),
    Circ(Circle),
    Rect(Rectangle),
    RoundRect(RoundedRectangle),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct GamObject {
    pub canvas: Gid,
    pub obj: GamObjectType,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct SetCanvasBoundsRequest {
    pub canvas: Gid,
    pub requested: Point,
    pub granted: Option<Point>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ContentCanvasRequest {
    // return value of the canvas Gid
    pub canvas: Gid,
    // name of the server requesting the content canvas
    pub servername: String<256>,
    // redraw message scalar ID - to be sent back to the requestor in case a redraw is required
    pub redraw_scalar_id: usize,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    // clears a canvas with a given GID
    ClearCanvas, //(Gid),

    // return the dimensions of a canvas as a Point (the top left is always (0,0))
    GetCanvasBounds, //(Gid),

    // request a new size for my canvas.
    // This normally will be denied, unless the requested Gid corresponds to a special canvas that allows resizing.
    SetCanvasBounds, //(SetCanvasBoundsRequest),

    // draws an object
    RenderObject, //(GamObject),

    // renders a TextView
    RenderTextView, //(TextView),

    // forces a redraw (which also does defacement, etc.)
    Redraw,

    // returns a GID to the "content" Canvas; currently, anyone can request it and draw to it, but maybe that policy should be stricter.
    // the Gid argument is the rkyv return value.
    RequestContentCanvas, //(ContentCanvasRequest),

    // Requests setting the UI to the power down screen
    PowerDownRequest,

    // Request blank screen for ship mode
    ShipModeBlankRequest,

    /////// planned

    // hides a canvas with a given GID
    //HideCanvas(Gid),

    // indicates if the current UI layout requires an input field
    //HasInput(bool),
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Return {
    RenderReturn(TextView),
    SetCanvasBoundsReturn(SetCanvasBoundsRequest),
    ContentCanvasReturn(ContentCanvasRequest),
    Failure,
}
