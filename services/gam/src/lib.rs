#![cfg_attr(target_os = "none", no_std)]

pub mod api;
use api::*;

use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Point, Gid, Line, Rectangle, Circle, RoundedRectangle, TokenClaim};

use api::Opcode; // if you prefer to map the api into your local namespace
use xous::{send_message, CID, Message};
use xous_ipc::{String, Buffer};
use num_traits::ToPrimitive;

#[derive(Debug)]
pub struct Gam {
    conn: CID,
}
impl Gam {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.store(REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(api::SERVER_NAME_GAM).expect("Can't connect to GAM");
        Ok(Gam {
          conn,
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

    pub fn request_content_canvas(&self, requestor_name: &str, redraw_id: usize) -> Result<Gid, xous::Error> {
        let mut server = String::<256>::new();
        use core::fmt::Write;
        write!(server, "{}", requestor_name).expect("GAM_API: couldn't write request_content_canvas server name");
        let req = ContentCanvasRequest {
            canvas: Gid::new([0,0,0,0]),
            servername: server,
            redraw_scalar_id: redraw_id,
        };
        let mut buf = Buffer::into_buf(req).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::RequestContentCanvas.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        match buf.to_original().unwrap() {
            api::Return::ContentCanvasReturn(ret) => {
                Ok(ret.canvas)
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
    pub fn allow_less_trusted_code(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AllowLessTrustedCode.to_usize().unwrap(), 0, 0, 0, 0)
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
}

use core::sync::atomic::{AtomicU32, Ordering};
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Gam {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.conn).unwrap();}
        }
    }
}
