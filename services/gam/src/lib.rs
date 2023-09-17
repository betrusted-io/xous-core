#![cfg_attr(target_os = "none", no_std)]

//! Detailed docs are parked under Structs/Gam down below

pub mod api;
pub use api::*;
pub mod modal;
pub use modal::*;
pub mod menu;
pub use menu::*;
pub mod apps;
pub use apps::*;
#[cfg(feature="ditherpunk")]
pub mod bitmap;
#[cfg(feature="ditherpunk")]
pub use bitmap::{Bitmap, Img, PixelType, DecodePng};

pub use graphics_server::api::{TextOp, TextView};
pub use graphics_server::api::{Gid, Line, Circle, RoundedRectangle, TokenClaim};
pub use graphics_server::api::{Point, Rectangle};
#[cfg(feature="ditherpunk")]
pub use graphics_server::api::Tile;
pub use graphics_server::api::GlyphStyle;
pub use graphics_server::api::PixelColor;
use api::Opcode; // if you prefer to map the api into your local namespace
use xous::{send_message, CID, Message};
use xous_ipc::{String, Buffer};
use num_traits::*;

use ime_plugin_api::{ImefCallback, ApiToken};

#[doc = include_str!("../README.md")]

pub const SYSTEM_STYLE: GlyphStyle = GlyphStyle::Tall;

// Add names here and insert them into the EXPECTED_BOOT_CONTEXTS structure below.
pub const MAIN_MENU_NAME: &'static str = "main menu";
pub const PDDB_MODAL_NAME: &'static str = "pddb modal";
pub const PDDB_MENU_NAME: &'static str = "pddb menu";
pub const ROOTKEY_MODAL_NAME: &'static str = "rootkeys modal";
pub const EMOJI_MENU_NAME: &'static str = "emoji menu";
pub const SHARED_MODAL_NAME: &'static str = "shared modal";
pub const STATUS_BAR_NAME: &'static str = "status";
pub const APP_NAME_SHELLCHAT: &'static str = "shellchat";
pub const APP_MENU_NAME: &'static str = "app menu";
pub const WIFI_MENU_NAME: &'static str = "WLAN menu";
pub const PREFERENCES_MENU_NAME: &'static str = "Preferences menu";

/// UX context registry. Names here are authorized by the GAM to have Canvases.
pub const EXPECTED_BOOT_CONTEXTS: &[&'static str] = &[
    APP_NAME_SHELLCHAT,
    MAIN_MENU_NAME,
    STATUS_BAR_NAME,
    EMOJI_MENU_NAME,
    ROOTKEY_MODAL_NAME,
    PDDB_MODAL_NAME,
    SHARED_MODAL_NAME,
    PDDB_MENU_NAME,
    APP_MENU_NAME,
    WIFI_MENU_NAME,
    PREFERENCES_MENU_NAME,
];

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum FocusState {
    Background = 0,
    Foreground = 1,
}
impl FocusState {
    pub fn convert_focus_change(code: usize) -> FocusState {
        if code != 0 {
            FocusState::Foreground
        } else {
            FocusState::Background
        }
    }
}


#[derive(Debug)]
pub struct Gam {
    /// The Gam structure exists on the client-side. This is the connection ID to the GAM server, local to this client.
    conn: xous::CID,
    /// A SID for callbacks from the GAM (e.g. redraw requests)
    callback_sid: Option<xous::SID>,
}
impl Gam {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_GAM).expect("Can't connect to GAM");
        Ok(Gam {
          conn,
          callback_sid: None,
        })
    }
    pub fn conn(&self) -> CID { self.conn }
    pub fn getop_revert_focus(&self) -> u32 { // non-blocking version is handed out to the menu handler
        Opcode::RevertFocusNb.to_u32().unwrap()
    }
    pub fn redraw(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }

    /// Inform the GAM that the main menu can be activated. This blocks until the message has been delivered to the GAM.
    pub fn allow_mainmenu(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AllowMainMenu.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_|())
    }

    pub fn powerdown_request(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::PowerDownRequest.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(confirmed) = response {
            if confirmed != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            panic!("GAM_API: unexpected return value: {:#?}", response);
        }
    }
    pub fn shipmode_blank_request(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::ShipModeBlankRequest.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(confirmed) = response {
            if confirmed != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            panic!("GAM_API: unexpected return value: {:#?}", response);
        }
    }

    /// this "posts" a textview -- it's not a "draw" as the update is neither guaranteed nor instantaneous
    /// the GAM first has to check that the textview is allowed to be updated, and then it will decide when
    /// the actual screen update is allowed
    pub fn post_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        tv.set_op(TextOp::Render);
        // force the clip_rect to none, in case a stale value from a previous bounds computation was hanging out
        // the bounds should /always/ come from the GAM canvas when doing a "live fire" redraw
        tv.clip_rect = None;
        let mut buf = Buffer::into_buf(tv.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RenderTextView.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::RenderReturn(tvr) => {
                tv.bounds_computed = tvr.bounds_computed;
                tv.cursor = tvr.cursor;
            }
            api::Return::NotCurrentlyDrawable => {
                tv.bounds_computed = None;
            }
            _ => panic!("GAM_API: post_textview got a return value from the server that isn't expected or handled")
        }
        tv.set_op(TextOp::Nop);
        Ok(())
    }
    /// Bounds computation does no checks on security since it's a non-drawing operation. While normal drawing always
    /// takes the bounds from the canvas, the caller can specify a clip_rect in this tv, instead of drawing the
    /// clip_rect from the Canvas associated with the tv.
    pub fn bounds_compute_textview(&self, tv: &mut TextView) -> Result<(), xous::Error> {
        tv.set_op(TextOp::ComputeBounds);
        let mut buf = Buffer::into_buf(tv.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RenderTextView.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::RenderReturn(tvr) => {
                tv.bounds_computed = tvr.bounds_computed;
                tv.cursor = tvr.cursor;
            }
            _ => panic!("GAM_API: bounds_compute_textview got a return value from the server that isn't expected or handled")
        }
        tv.set_op(TextOp::Nop);
        Ok(())
    }

    pub fn draw_line(&self, gid: Gid, line: Line) -> Result<(), xous::Error> {
        let go = GamObject {
            canvas: gid,
            obj: GamObjectType::Line(line),
        };
        let buf = Buffer::into_buf(go).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObject.to_u32().unwrap()).map(|_|())
    }
    pub fn draw_rectangle(&self, gid: Gid, rect: Rectangle) -> Result<(), xous::Error> {
        let go = GamObject {
            canvas: gid,
            obj: GamObjectType::Rect(rect),
        };
        log::trace!("draw_rectangle: {:?}, conn: {}", go, self.conn);
        let buf = Buffer::into_buf(go).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObject.to_u32().unwrap()).map(|_|())
    }
    pub fn draw_rounded_rectangle(&self, gid: Gid, rr: RoundedRectangle) -> Result<(), xous::Error> {
        let go = GamObject {
            canvas: gid,
            obj: GamObjectType::RoundRect(rr),
        };
        let buf = Buffer::into_buf(go).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObject.to_u32().unwrap()).map(|_|())
    }
    #[cfg(feature="ditherpunk")]
    pub fn draw_bitmap(&self, gid: Gid, bm: &Bitmap) -> Result<(), xous::Error> {
        for (_i, tile) in bm.iter().enumerate(){
            let gt = GamTile {
                tile: *tile,
                canvas: gid,
            };
            let buf = Buffer::into_buf(gt).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::RenderTile.to_u32().unwrap())
                .map(|_| ())?;
        };
        Ok(())
    }
    pub fn draw_circle(&self, gid: Gid, circ: Circle) -> Result<(), xous::Error> {
        let go = GamObject {
                canvas: gid,
                obj: GamObjectType::Circ(circ),
        };
        let buf = Buffer::into_buf(go).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObject.to_u32().unwrap()).map(|_|())
    }
    pub fn draw_list(&self, list: GamObjectList) -> Result<(), xous::Error> {
        let buf = Buffer::into_buf(list).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObjectList.to_u32().unwrap())
            .map(|_| ())
    }

    pub fn get_canvas_bounds(&self, gid: Gid) -> Result<Point, xous::Error> {
        log::trace!("GAM_API: get_canvas_bounds");
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::GetCanvasBounds.to_usize().unwrap(),
                gid.gid()[0] as _,  gid.gid()[1] as _,  gid.gid()[2] as _,  gid.gid()[3] as _))
                .expect("GAM_API: can't get canvas bounds from GAM");
            if let xous::Result::Scalar2(tl, br) = response {
            // note that the result should always be normalized so the rectangle's "tl" should be (0,0)
            log::trace!("GAM_API: tl:{:?}, br:{:?}", Point::from(tl), Point::from(br));
            assert!(tl == 0, "GAM_API: api call returned non-zero top left for canvas bounds");
            Ok(br.into())
        } else {
            panic!("GAM_API: can't get canvas bounds")
        }
    }

    pub fn set_canvas_bounds_request(&self, req: &mut SetCanvasBoundsRequest) -> Result<(), xous::Error> {
        let mut buf = Buffer::into_buf(req.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::SetCanvasBounds.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        match buf.to_original().unwrap() {
            api::Return::SetCanvasBoundsReturn(ret) => {
                req.granted = ret.granted;
            }
            _ => panic!("GAM_API: set_canvas_bounds_request view got a return value from the server that isn't expected or handled")
        }
        Ok(())
    }

    pub fn request_content_canvas(&self, token: [u32; 4]) -> Result<Gid, xous::Error> {
        let mut buf = Buffer::into_buf(token).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RequestContentCanvas.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::ContentCanvasReturn(ret) => {
                if let Some(gid) = ret {
                    Ok(gid)
                } else {
                    Err(xous::Error::InternalError)
                }
            }
            _ => {
                log::error!("GAM_API: request_content_canvas got a return value from the server that isn't expected or handled");
                Err(xous::Error::InternalError)
            }
        }
    }

    pub fn type_chars(&self, s: &str) -> Result<bool, xous::Error> {
        let mut view = s.chars().peekable();

        while view.peek().is_some() {
            let chunk: std::string::String = view.by_ref().take(4).collect();
            send_message(self.conn,
                Message::new_scalar(Opcode::KeyboardEvent.to_usize().unwrap(),
                 chunk.chars().nth(0).unwrap() as usize,
                 if chunk.len() > 1 {chunk.chars().nth(1).unwrap()} else {'\u{0000}'} as usize,
                 if chunk.len() > 2 {chunk.chars().nth(2).unwrap()} else {'\u{0000}'} as usize,
                 if chunk.len() > 3 {chunk.chars().nth(3).unwrap()} else {'\u{0000}'} as usize
                )
            ).expect("Couldn't type chars");
        }

        Ok(true)
    }

    pub fn claim_token(&self, name: &str) -> Result<Option<[u32; 4]>, xous::Error> {
        let tokenclaim = TokenClaim {
            token: None,
            name: String::<128>::from_str(name),
        };
        let mut buf = Buffer::into_buf(tokenclaim).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::ClaimToken.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let returned_claim = buf.to_original::<TokenClaim, _>().unwrap();

        Ok(returned_claim.token)
    }
    pub fn set_predictor_api_token(&self, api_token: [u32; 4], gam_token: [u32; 4]) -> Result<(), xous::Error> {
        let at = ApiToken {
            gam_token,
            api_token,
        };
        let buf = Buffer::into_buf(at).or(Err(xous::Error::InternalError))?;
        buf.send(self.conn, Opcode::PredictorApiToken.to_u32().unwrap())
        .or(Err(xous::Error::InternalError)).map(|_|())
    }
    pub fn trusted_init_done(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::TrustedInitDone.to_usize().unwrap(), 0, 0, 0, 0)
        ).expect("couldn't run allow trusted code check");
        if let xous::Result::Scalar1(result) = response {
            if result == 1 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }

    pub fn register_ux(&self, registration: UxRegistration) -> Result<Option<[u32; 4]>, xous::Error> {
        let mut buf = Buffer::into_buf(registration).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RegisterUx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::UxToken(token) => {
                Ok(token)
            }
            _ => {
                Err(xous::Error::InternalError)
            }
        }
    }

    pub fn set_audio_opcode(&self, opcode: u32, token: [u32; 4]) -> Result<(), xous::Error> {
        let audio_op = SetAudioOpcode {
            token,
            opcode,
        };
        let buf = Buffer::into_buf(audio_op).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::SetAudioOpcode.to_u32().unwrap()).or(Err(xous::Error::InternalError)).map(|_| ())
    }

    pub fn set_vibe(&self, enable: bool) -> Result<(), xous::Error> {
        let ena =
            if enable { 1 }
            else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::Vibe.to_usize().unwrap(),
            ena, 0, 0, 0,)
        ).map(|_| ())
    }
    pub fn toggle_menu_mode(&self, token: [u32; 4]) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::ToggleMenuMode.to_usize().unwrap(),
            token[0] as usize,
            token[1] as usize,
            token[2] as usize,
            token[3] as usize,
            )
        ).map(|_| ())
    }
    /// this indicates to the GAM that the currently running app no longer wants to be the focus of attention
    /// we might respect that. or maybe not. depends on the GAM's policies.
    pub fn relinquish_focus(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::RevertFocus.to_usize().unwrap(),
            0, 0, 0, 0,)
        ).map(|_| ())
    }
    /// note to self: this call isn't actually used - it might come in handy to debug a problem, but
    /// in general if a context isn't redrawing, this isn't the root cause.
    pub fn relinquish_focus_nb(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RevertFocusNb.to_usize().unwrap(),
            0, 0, 0, 0,)
        ).map(|_| ())
    }

    pub fn glyph_height_hint(&self, glyph: GlyphStyle) -> Result<usize, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::QueryGlyphProps.to_usize().unwrap(),
            glyph as usize, 0, 0, 0,)
        ).expect("QueryGlyphProps failed");
        if let xous::Result::Scalar1(h) = response {
            Ok(h)
        } else {
            panic!("unexpected return value: {:#?}", response);
        }
    }
    pub fn request_ime_redraw(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::RedrawIme.to_usize().unwrap(),
            0, 0, 0, 0,)
        ).map(|_| ())
    }
    /// requests a context switch to a new app. Doesn't have to be obeyed, depending
    /// upon the GAM's policy. Only the certain sources can switch apps, hence the token.
    /// Failure to switch *is silent* because waiting for an activation to go through and
    /// confirm can cause a deadlock condition.
    pub fn switch_to_app(&self, app_name: &str, token: [u32; 4]) -> Result<(), xous::Error> {
        let switchapp = SwitchToApp {
            token,
            app_name: String::<128>::from_str(app_name),
        };
        let buf = Buffer::into_buf(switchapp).or(Err(xous::Error::InternalError))?;
        buf.send(self.conn, Opcode::SwitchToApp.to_u32().unwrap()).or(Err(xous::Error::InternalError)).map(|_|())
    }
    pub fn raise_menu(&self, menu_name_str: &str) -> Result<(), xous::Error> {
        let menu_name = GamActivation {
            name: String::<128>::from_str(menu_name_str),
            result: None,
        };
        let mut buf = Buffer::into_buf(menu_name).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RaiseMenu.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("couldn't send RaiseMenu opcode");
        let result = buf.to_original::<GamActivation, _>().unwrap();
        if let Some(code) = result.result {
            match code {
                ActivationResult::Success => Ok(()),
                ActivationResult::Failure => {
                    log::warn!("Couldn't raise {}", menu_name_str);
                    Err(xous::Error::ShareViolation)
                }
            }
        } else {
            Err(xous::Error::InternalError)
        }
    }
    pub fn raise_modal(&self, modal_name: &str) -> Result<(), xous::Error> {
        self.raise_menu(modal_name)
    }
    /// this is a one-way door, once you've set it, you can't unset it.
    pub fn set_devboot(&self, enable: bool) -> Result<(), xous::Error> {
        let ena =
            if enable { 1 }
            else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::Devboot.to_usize().unwrap(),
            ena, 0, 0, 0,)
        ).map(|_| ())
    }
    pub fn selftest(&self, duration_ms: usize) {
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::TestPattern.to_usize().unwrap(), duration_ms, 0, 0, 0),
        )
        .expect("couldn't self test");
    }
    pub fn set_debug_level(&self, level: log::LevelFilter) {
        let l: usize = match level {
            log::LevelFilter::Debug => 1,
            log::LevelFilter::Trace => 2,
            _ => 0,
        };
        send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::SetDebugLevel.to_usize().unwrap(), l, 0, 0, 0),
        )
        .expect("couldn't set debug level");
    }
    pub fn bytes_to_bip39(&self, bytes: &Vec::<u8>) -> Result<Vec::<std::string::String>, xous::Error> {
        match bytes.len() {
            16 | 20 | 24 | 28 | 32 => (),
            _ => return Err(xous::Error::InvalidString)
        }
        let mut ipc = Bip39Ipc::default();
        ipc.data[..bytes.len()].copy_from_slice(&bytes);
        ipc.data_len = bytes.len() as u32;
        let mut buf = Buffer::into_buf(ipc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::BytestoBip39.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("couldn't send RaiseMenu opcode");
        let result = buf.to_original::<Bip39Ipc, _>().unwrap();
        let mut ret = Vec::<std::string::String>::new();
        for word in result.words {
            if let Some(w) = word {
                ret.push(w.as_str().unwrap().to_string());
            }
        }
        if ret.len() == 0 {
            Err(xous::Error::InvalidString)
        } else {
            Ok(ret)
        }
    }
    pub fn bip39_to_bytes(&self, bip39: &Vec::<std::string::String>) -> Result<Vec::<u8>, xous::Error> {
        match bip39.len() {
            12 | 15 | 18 | 21 | 24 => (),
            _ => return Err(xous::Error::InvalidString)
        }
        let mut ipc = Bip39Ipc::default();
        for (word, slot) in bip39.iter().zip(ipc.words.iter_mut()) {
            *slot = Some(xous_ipc::String::from_str(word))
        }
        let mut buf = Buffer::into_buf(ipc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::Bip39toBytes.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("couldn't send RaiseMenu opcode");
        let result = buf.to_original::<Bip39Ipc, _>().unwrap();
        if result.data_len == 0 {
            Err(xous::Error::InvalidString)
        } else {
            Ok(
                result.data[..result.data_len as usize].to_vec()
            )
        }
    }
    pub fn bip39_suggestions(&self, start: &str) -> Result<Vec::<std::string::String>, xous::Error> {
        let mut ipc = Bip39Ipc::default();
        // we abuse this struct a bit by shoving the lookup phrase into a u8-array...
        let checked_start = start.as_bytes();
        ipc.data[..checked_start.len().min(8)].copy_from_slice(&checked_start[..checked_start.len().min(8)]);
        ipc.data_len = checked_start.len().min(8) as u32;
        let mut buf = Buffer::into_buf(ipc).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::Bip39Suggestions.to_u32().unwrap()).or(Err(xous::Error::InternalError)).expect("couldn't send RaiseMenu opcode");
        let result = buf.to_original::<Bip39Ipc, _>().unwrap();
        let mut suggestions = Vec::<std::string::String>::new();
        for word in result.words {
            if let Some(w) = word {
                suggestions.push(w.as_str().unwrap().to_string())
            }
        }
        Ok(suggestions)
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Gam {
    fn drop(&mut self) {
        if let Some(sid) = self.callback_sid.take() {
            // no need to tell the pstream server we're quitting: the next time a callback processes,
            // it will automatically remove my entry as it will receive a ServerNotFound error.

            // tell my handler thread to quit
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_blocking_scalar(ImefCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
            xous::destroy_server(sid).unwrap();
        }
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}

// common message forwarding infrastructure used by Menus, Modals, etc...
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct MsgForwarder {
    pub public_sid: [u32; 4],
    pub private_sid: [u32; 4],
    pub redraw_op: u32,
    pub rawkeys_op: u32,
    pub drop_op: u32,
}
/// this is a simple server that forwards incoming messages from a generic
/// "modal" interface to the internal private server. It keeps the GAM from being
/// able to tinker with the internal mechanics of the larger server that owns the
/// dialog box.
fn forwarding_thread(addr: usize, size: usize, offset: usize) {
    let buf = unsafe{Buffer::from_raw_parts(addr, size, offset)};
    let forwarding_config = buf.to_original::<MsgForwarder, _>().unwrap();
    let private_conn = xous::connect(xous::SID::from_array(forwarding_config.private_sid)).expect("couldn't connect to the private server");

    log::trace!("modal forwarding server started");
    loop {
        let msg = xous::receive_message(xous::SID::from_array(forwarding_config.public_sid)).unwrap();
        log::trace!("modal forwarding server got msg: {:?}", msg);
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(ModalOpcode::Redraw) => {
                xous::send_message(private_conn,
                    Message::new_scalar(forwarding_config.redraw_op as usize, 0, 0, 0, 0)
                ).expect("couldn't forward redraw message");
            },
            Some(ModalOpcode::Rawkeys) => xous::msg_scalar_unpack!(msg, k1, k2, k3, k4, {
                xous::send_message(private_conn,
                    Message::new_scalar(forwarding_config.rawkeys_op as usize, k1, k2, k3, k4)
                ).expect("couldn't forard rawkeys message");
            }),
            Some(ModalOpcode::Quit) => {
                xous::send_message(private_conn,
                    Message::new_scalar(forwarding_config.drop_op as usize, 0, 0, 0, 0)
                ).expect("couldn't forward drop message");
                break;
            },
            None => {
                log::error!("unknown opcode {:?}", msg.body.id());
            }
        }
    }
    log::trace!("modal forwarding server exiting");
    xous::destroy_server(xous::SID::from_array(forwarding_config.public_sid)).expect("can't destroy my server on exit!");
}
