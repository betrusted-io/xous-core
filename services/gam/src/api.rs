mod rkyv_enum;
// note: many enums in the API are isolated to this file.
#[cfg(feature = "ditherpunk")]
use graphics_server::api::Tile;
use graphics_server::api::{Gid, Point};
pub use rkyv_enum::*;
use xous_ipc::String;

pub(crate) const SERVER_NAME_GAM: &str = "_Graphical Abstraction Manager_";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct GamObject {
    pub canvas: Gid,
    pub obj: GamObjectType,
}
#[cfg(feature = "ditherpunk")]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct GamTile {
    pub canvas: Gid,
    pub tile: Tile,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct GamObjectList {
    pub canvas: Gid,
    pub list: [Option<GamObjectType>; 32],
    free: usize,
}
impl GamObjectList {
    pub fn new(canvas: Gid) -> GamObjectList { GamObjectList { canvas, list: Default::default(), free: 0 } }

    pub fn push(&mut self, item: GamObjectType) -> Result<(), GamObjectType> {
        if self.free < self.list.len() {
            self.list[self.free] = Some(item);
            self.free += 1;
            Ok(())
        } else {
            Err(item)
        }
    }

    pub fn last(&self) -> Option<GamObjectType> {
        match self.free {
            0 => self.list[self.free],
            _ => self.list[self.free - 1],
        }
    }
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
    pub app_name: String<128>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub enum UxType {
    Chat,
    Menu,
    Modal,
    Framebuffer,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct UxRegistration {
    // request specification
    pub app_name: String<128>, /* the putative name of our application - GAM may modify this if a spoof
                                * attempt is detected */
    pub ux_type: UxType,
    pub predictor: Option<String<64>>, /* optional specification for an IME prediction engine to use. This
                                        * can be updated later on, or None and a default engine will be
                                        * provided. */

    // Callbacks:
    /// SID ofserver for callbacks from the GAM. Note this is a disclosure of the SID, which is normally a
    /// secret in the kernel services. however, for apps, we allow disclosure of this to the kernel
    /// services, because we trust them.
    pub listener: [u32; 4],
    /// opcode ID for redraw messages. This is mandatory.
    pub redraw_id: u32,
    /// optional opcode ID for inputs. If presented, input Strings are sent to this Ux
    pub gotinput_id: Option<u32>,
    /// optional opcode ID for audio frames. If presented, audio callbacks requests for more play/rec data
    /// will be sent directly to this opcode
    pub audioframe_id: Option<u32>,
    /// optional opcode ID for raw keystrokes. They are passed on to the caller in real-time.
    pub rawkeys_id: Option<u32>,
    /// optional opcode ID code for focus change notifications. Most applications will want to provide this
    /// to stop hogging resources when backgrounded If the LayoutType is not an App, this field is
    /// ignored and does nothing
    pub focuschange_id: Option<u32>,
}
#[cfg(feature = "unsafe-app-loading")]
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct NameRegistration {
    pub name: String<128>,
    pub auth_token: [u32; 4],
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    // clears a canvas with a given GID
    ClearCanvas = 0, //(Gid),

    // return the dimensions of a canvas as a Point (the top left is always (0,0))
    GetCanvasBounds = 1, //(Gid),

    // request a new size for my canvas.
    // This normally will be denied, unless the requested Gid corresponds to a special canvas that allows
    // resizing.
    SetCanvasBounds = 2, //(SetCanvasBoundsRequest),

    // draws an object
    RenderObject = 3, //(GamObject),
    RenderObjectList = 4,
    // draws a tile. this is *not* part of the ObjectList because then every vector object suddenly also
    // carries the allocation burden of a bitmap tile.
    #[cfg(feature = "ditherpunk")]
    RenderTile = 5,

    // renders a TextView
    RenderTextView = 6, //(TextView),

    // forces a redraw (which also does defacement, etc.)
    Redraw = 7,

    // returns a GID to the "content" Canvas of the token holder
    RequestContentCanvas = 8,

    // registers a Ux of a requested type
    // takes in the LayoutType, default PredictorType, a SID for UxEvents, a human-readable identifier
    // token; returns a content canvas GID also takes a bunch of optional ID codes for the various
    // callbacks internally assigns a trust level, based on a first-come first-serve basis for known
    // services, and then a much lower trust for rando ones
    RegisterUx = 9,

    // updates the audio connection ID post-registration
    SetAudioOpcode = 10,

    // Requests setting the UI to the power down screen
    PowerDownRequest = 11,

    // Request blank screen for ship mode
    ShipModeBlankRequest = 12,

    // used to claim a GAM registration token (should be used only by status.rs)
    ClaimToken = 13,
    // used to set a predictor API token (should be used only by ime-frontend)
    PredictorApiToken = 14,

    /// system-level API that can be called by the Xous process launcher to check if we're at a state where
    /// less trusted code could be run it basically checks that all tokens have been claimed by trusted
    /// OS procesess, thus blocking any further token creation
    TrustedInitDone = 15,

    /// this is used internally to route input lines from the IMEF
    InputLine = 16,

    /// passed to the keyboard server to notify me of a keyboard event
    KeyboardEvent = 17,

    /// used to turn keyboard vibrate on and off
    Vibe = 18,
    /// used to toggle menu mode behavior for the prediction area (default: false)
    ToggleMenuMode = 19,

    /// called by a context when it's done with taking the screen; requests the GAM to revert focus to the
    /// last-focused app
    RevertFocus = 20,
    RevertFocusNb = 21, // non-blocking version

    /// pass-through to get glyph heights to assist with layout planning, without having to create a gfx
    /// connection
    QueryGlyphProps = 22,

    /// request redraw of IME area
    RedrawIme = 23,

    /// switch focus to an app
    SwitchToApp = 24,

    /// raise a context menu
    RaiseMenu = 25,

    /// Turn on Devboot Flag
    Devboot = 26,

    /// Show a test pattern. Can only call this once (to prevent abuse)
    TestPattern = 27,

    /// Toggle debug on serial console
    SetDebugLevel = 28,

    Quit = 29,

    /// Bip39 operations -- the GAM has the word list, so to avoid duplicating code it offers a conversion
    /// service.
    Bip39toBytes = 30,
    BytestoBip39 = 31,
    Bip39Suggestions = 32,

    /// Allow main menu activation. Used by the PDDB to turn ungate the main menu once it is mounted.
    /// This resolves race conditions that depend upon the PDDB configurations.
    AllowMainMenu = 33,

    /// Register a name that can acquire a token. This is only intended to be used with pre-registered apps
    #[cfg(feature = "unsafe-app-loading")]
    RegisterName = 34,
}

// small wart -- we have to reset the size of a modal to max size for resize computations
// reveal the max size globally, since it's a constant
pub const MODAL_Y_MAX: i16 = 350; // in absolute screen coords, not relative to top pad

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, PartialEq, Eq)]
pub enum ActivationResult {
    Success,
    Failure,
}
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct GamActivation {
    pub(crate) name: xous_ipc::String<128>,
    pub(crate) result: Option<ActivationResult>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct MenuManagement {
    pub(crate) item: MenuItem,
    pub(crate) op: MenuMgrOp,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct MenuItem {
    pub name: String<64>,
    /// if action_conn is None, this is a NOP menu item (it just does nothing and closes the menu)
    pub action_conn: Option<xous::CID>,
    pub action_opcode: u32, // this is ignored if action_conn is None
    pub action_payload: MenuPayload,
    pub close_on_select: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CanvasType {
    ChatContent,
    ChatInput,
    ChatPreditive,
    Framebuffer,
    Modal,
    Menu,
    Status,
}
impl CanvasType {
    pub fn is_content(&self) -> bool {
        match &self {
            CanvasType::ChatContent
            | CanvasType::Framebuffer
            | CanvasType::Modal
            | CanvasType::Menu
            | CanvasType::Status => true,
            _ => false,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct GidRecord {
    pub gid: Gid,
    pub canvas_type: CanvasType,
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct Bip39Ipc {
    pub data: [u8; 32],
    pub data_len: u32,
    pub words: [Option<xous_ipc::String<8>>; 24],
}
