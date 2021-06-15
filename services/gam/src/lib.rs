#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Point, Gid, Line, Rectangle, Circle, RoundedRectangle, TokenClaim};

use api::Opcode; // if you prefer to map the api into your local namespace
use xous::{send_message, CID, Message};
use xous_ipc::{String, Buffer};
use num_traits::ToPrimitive;

use ime_plugin_api::ImefCallback;

#[derive(Debug)]
pub struct Gam {
    conn: CID,
    callback_sid: Option<xous::SID>,
}
impl Gam {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_GAM).expect("Can't connect to GAM");
        Ok(Gam {
          conn,
          callback_sid: None,
        })
    }

    pub fn redraw(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Redraw.to_usize().unwrap(), 0, 0, 0, 0)
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
        let mut buf = Buffer::into_buf(tv.clone()).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RenderTextView.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::RenderReturn(tvr) => {
                tv.bounds_computed = tvr.bounds_computed;
                tv.cursor = tvr.cursor;
            }
            _ => panic!("GAM_API: post_textview got a return value from the server that isn't expected or handled")
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
    pub fn draw_circle(&self, gid: Gid, circ: Circle) -> Result<(), xous::Error> {
        let go = GamObject {
                canvas: gid,
                obj: GamObjectType::Circ(circ),
        };
        let buf = Buffer::into_buf(go).or(Err(xous::Error::InternalError))?;
        buf.lend(self.conn, Opcode::RenderObject.to_u32().unwrap()).map(|_|())
    }

    pub fn get_canvas_bounds(&self, gid: Gid) -> Result<Point, xous::Error> {
        log::trace!("GAM_API: get_canvas_bounds");
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::GetCanvasBounds.to_usize().unwrap(),
                gid.gid()[0] as _,  gid.gid()[1] as _,  gid.gid()[2] as _,  gid.gid()[3] as _))
                .expect("GAM_API: can't get canvas bounds from GAM");
            if let xous::Result::Scalar2(tl, br) = response {
            // note that the result should always be normalized so the rectangle's "tl" should be (0,0)
            log::trace!("GAM_API: tl:{}, br:{}", tl, br);
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
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
