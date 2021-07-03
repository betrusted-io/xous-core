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

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq)]
pub enum TokenType {
    /// GAM tokens are for objects that the GAM delegates to do app logic.
    /// this is different to prevent delegated apps from masquerading as the app itself
    Gam,
    /// App token is a token given to the app and only the app to identify itself to the Gam
    /// for any requests
    App,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct SetCanvasBoundsRequest {
    pub token: [u32; 4],
    pub token_type: TokenType,
    pub requested: Point,
    pub granted: Option<Point>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct SetAudioOpcode {
    pub token: [u32; 4],
    pub opcode: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct SwitchToApp {
    pub token: [u32; 4],
    pub app_name: String::<128>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum UxType {
    Chat,
    Menu,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UxRegistration {
    // request specification
    pub app_name: String::<128>,  // the putative name of our application - GAM may modify this if a spoof attempt is detected
    pub ux_type: UxType,
    pub predictor: Option<String::<64>>, // optional specification for an IME prediction engine to use. This can be updated later on, or None and a default engine will be provided.

    // Callbacks:
    /// SID ofserver for callbacks from the GAM. Note this is a disclosure of the SID, which is normally a secret in the kernel services.
    /// however, for apps, we allow disclosure of this to the kernel services, because we trust them.
    pub listener: [u32; 4],
    /// opcode ID for redraw messages. This is mandatory.
    pub redraw_id: u32,
    /// optional opcode ID for inputs. If presented, input Strings are sent to this Ux
    pub gotinput_id: Option<u32>,
    /// optional opcode ID for audio frames. If presented, audio callbacks requests for more play/rec data will be sent directly to this opcode
    pub audioframe_id: Option<u32>,
    /// optional opcode ID for raw keystrokes. They are passed on to the caller in real-time.
    pub rawkeys_id: Option<u32>,
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

    // returns a GID to the "content" Canvas of the token holder
    RequestContentCanvas,

    // registers a Ux of a requested type
    // takes in the LayoutType, default PredictorType, a SID for UxEvents, a human-readable identifier token; returns a content canvas GID
    // also takes a bunch of optional ID codes for the various callbacks
    // internally assigns a trust level, based on a first-come first-serve basis for known services, and then a much lower trust for rando ones
    RegisterUx,

    // updates the audio connection ID post-registration
    SetAudioOpcode,

    // Requests setting the UI to the power down screen
    PowerDownRequest,

    // Request blank screen for ship mode
    ShipModeBlankRequest,

    // used to claim a GAM registration token (should be used only by status.rs)
    ClaimToken,

    /// system-level API that can be called by the Xous process launcher to check if we're at a state where less trusted code could be run
    /// it basically checks that all tokens have been claimed by trusted OS procesess, thus blocking any further token creation
    TrustedInitDone,

    /// this is used internally to route input lines from the IMEF
    InputLine,

    /// passed to the keyboard server to notify me of a keyboard event
    KeyboardEvent,

    /// used to turn keyboard vibrate on and off
    Vibe,

    /// called by a context when it's done with taking the screen; requests the GAM to revert focus to the last-focused app
    RevertFocus,
    RevertFocusNb, // non-blocking version

    /// request an app to take the focus
    RequestFocus,

    /// pass-through to get glyph heights to assist with layout planning, without having to create a gfx connection
    QueryGlyphProps,

    /// request redraw of IME area
    RedrawIme,

    /// switch focus to an app
    SwitchToApp,

    /// raise a context menu
    RaiseMenu,

    /// Turn on Devboot Flag
    Devboot,

    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum Return {
    UxToken(Option<[u32; 4]>),
    RenderReturn(TextView),
    SetCanvasBoundsReturn(SetCanvasBoundsRequest),
    ContentCanvasReturn(Option<Gid>),
    Failure,
}
