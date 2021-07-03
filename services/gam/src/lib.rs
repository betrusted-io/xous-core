#![cfg_attr(target_os = "none", no_std)]

pub mod api;
pub use api::*;

use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Point, Gid, Line, Rectangle, Circle, RoundedRectangle, TokenClaim};
pub use graphics_server::GlyphStyle;
// menu imports
use graphics_server::api::{PixelColor, TextBounds, DrawStyle};

use api::Opcode; // if you prefer to map the api into your local namespace
use xous::{send_message, CID, Message};
use xous_ipc::{String, Buffer};
use num_traits::*;

use ime_plugin_api::ImefCallback;

#[derive(Debug)]
pub struct Gam {
    conn: xous::CID,
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
    pub fn conn(&self) -> CID { self.conn }
    pub fn getop_revert_focus(&self) -> u32 { // non-blocking version is handed out to the menu handler
        Opcode::RevertFocusNb.to_u32().unwrap()
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

    pub fn set_vibe(&self, enable: bool) -> Result<(), xous::Error> {
        let ena =
            if enable { 1 }
            else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::Vibe.to_usize().unwrap(),
            ena, 0, 0, 0,)
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
    pub fn request_focus(&self, token: [u32; 4]) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::RequestFocus.to_usize().unwrap(),
            token[0] as usize, token[1] as usize, token[2] as usize, token[3] as usize,)
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
    /// upon the GAM's policy. Only the main menu can switch apps, hence the token.
    pub fn switch_to_app(&self, app_name: &str, token: [u32; 4]) -> Result<(), xous::Error> {
        let switchapp = SwitchToApp {
            token,
            app_name: String::<128>::from_str(app_name),
        };
        let buf = Buffer::into_buf(switchapp).or(Err(xous::Error::InternalError))?;
        buf.send(self.conn, Opcode::SwitchToApp.to_u32().unwrap()).or(Err(xous::Error::InternalError)).map(|_|())
    }
    pub fn raise_menu(&self, menu_name: &str) -> Result<(), xous::Error> {
        let menu_name = String::<128>::from_str(menu_name);
        let buf = Buffer::into_buf(menu_name).or(Err(xous::Error::InternalError))?;
        buf.send(self.conn, Opcode::RaiseMenu.to_u32().unwrap()).or(Err(xous::Error::InternalError)).map(|_|())
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


//////////////
// menu structures
// couldn't figure out for the life of me how to get this into a file that wasn't lib.rs
// so here it is.

const MAX_ITEMS: usize = 16;

#[allow(dead_code)] // here until Memory types are implemented
#[derive(Debug, Copy, Clone)]
pub enum MenuPayload {
    /// memorized scalar payload
    Scalar([u32; 4]),
    /// this a nebulous-but-TBD maybe way of bodging in a more complicated record, which would involve
    /// casting this memorized, static payload into a Buffer and passing it on. Let's not worry too much about it for now, it's mostly apirational...
    Memory(([u8; 256], usize)),
}
#[derive(Debug, Copy, Clone)]
pub struct MenuItem {
    pub name: String::<64>,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: MenuPayload,
    pub close_on_select: bool,
}

#[derive(Debug)]
pub struct Menu {
    pub sid: xous::SID,
    pub gam: Gam,
    pub xns: xous_names::XousNames,
    pub items: [Option<MenuItem>; MAX_ITEMS],
    pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
    pub divider_margin: i16,
    pub line_height: i16,
    pub canvas_width: Option<i16>,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum MenuOpcode {
    Redraw,
    Rawkeys,
    Quit,
}

impl Menu {
    pub fn new(name: &str) -> Menu {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private menu message server");
        let gam = Gam::new(&xns).expect("can't connect to GAM");
        let authtoken = gam.register_ux(
            UxRegistration {
                app_name: String::<128>::from_str(name),
                ux_type: UxType::Menu,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: MenuOpcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: Some(MenuOpcode::Rawkeys.to_u32().unwrap()),
            }
        ).expect("couldn't register my Ux element with GAM");
        assert!(authtoken.is_some(), "Couldn't register menu. Did you remember to add the app_name to the tokens.rs expected boot contexts list?");
        log::debug!("requesting content canvas for menu");
        let canvas = gam.request_content_canvas(authtoken.unwrap()).expect("couldn't get my content canvas from GAM");
        let line_height = gam.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height hint") as i16;
        Menu {
            sid,
            gam,
            xns,
            items: [None; MAX_ITEMS],
            index: 0,
            canvas,
            authtoken: authtoken.unwrap(),
            margin: 4,
            divider_margin: 20,
            line_height,
            canvas_width: None,
        }
    }
    // if successful, returns None, otherwise, the menu item
    pub fn add_item(&mut self, new_item: MenuItem) -> Option<MenuItem> {
        // first, do the insertion.
        // add the menu item to the first free slot
        // any modifications to the menu structure should guarantee that the list is compacted
        // and has no holes, in order for the "selected index" logic to work
        let mut success = false;
        for item in self.items.iter_mut() {
            if item.is_none() {
                *item = Some(new_item);
                success = true;
                break;
            }
        }

        if success {
            // now, recompute the height
            let mut total_items = self.num_items();
            if total_items == 0 {
                total_items = 1; // just so we see a blank menu at least, and have a clue how to debug
            }
            let current_bounds = self.gam.get_canvas_bounds(self.canvas).expect("couldn't get current bounds");
            let mut new_bounds = SetCanvasBoundsRequest {
                requested: Point::new(current_bounds.x, total_items as i16 * self.line_height + self.margin * 2),
                granted: None,
                token_type: TokenType::App,
                token: self.authtoken,
            };
            log::debug!("add_item requesting bounds of {:?}", new_bounds);
            self.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");

            None
        } else {
            Some(new_item)
        }
    }
    pub fn draw_item(&self, index: i16, with_marker: bool) {
        use core::fmt::Write;
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();

        if let Some(item) = self.items[index as usize] {
            let mut item_tv = TextView::new(
                self.canvas,
                TextBounds::BoundingBox(Rectangle::new(
                    Point::new(self.margin, index * self.line_height + self.margin),
                    Point::new(canvas_size.x - self.margin, (index + 1) * self.line_height + self.margin),
                )));

            if with_marker {
                write!(item_tv.text, " • ").unwrap();
            } else {
                write!(item_tv.text, "    ").unwrap();
            }
            write!(item_tv.text, "{}", item.name.as_str().unwrap()).unwrap();
            item_tv.draw_border = false;
            item_tv.style = GlyphStyle::Small;
            item_tv.margin = Point::new(0, 0);
            item_tv.ellipsis = true;

            self.gam.post_textview(&mut item_tv).expect("couldn't render menu list item");
        }
    }
    // draw a dividing line above the indexed item
    pub fn draw_divider(&self, index: i16) {
        if let Some(canvas_width) = self.canvas_width {
            self.gam.draw_line(self.canvas, Line::new_with_style(
                Point::new(self.divider_margin, index * self.line_height + self.margin/2),
                Point::new(canvas_width - self.divider_margin, index * self.line_height + self.margin/2),
                DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1))
                ).expect("couldn't draw dividing line")
        } else {
            log::debug!("cant draw divider because our canvas width was not initialized. Ignoring request.");
        }
    }
    pub fn prev_item(&mut self) {
        if self.index > 0 {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index -= 1;
            // add the marker to the pervious item
            self.draw_item(self.index as i16, true);

            if self.index != 0 {
                self.draw_divider(self.index as i16);
            }
            if self.index < self.num_items() - 1 {
                self.draw_divider(self.index as i16 + 1);
            }
            if self.index < self.num_items() - 2 {
                self.draw_divider(self.index as i16 + 2);
            }
        }
    }
    pub fn next_item(&mut self) {
        if self.index < (self.num_items() - 1) {
            // wipe out the current marker
            self.draw_item(self.index as i16, false);
            self.index += 1;
            // add the marker to the pervious item
            self.draw_item(self.index as i16, true);

            if self.index != 1 {
                self.draw_divider(self.index as i16 - 1);
            }
            self.draw_divider(self.index as i16);
            if self.index < self.num_items() - 1 {
                self.draw_divider(self.index as i16 + 1);
            }
        }
    }
    pub fn redraw(&mut self) {
        // for now, just draw a black rectangle
        log::trace!("menu redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        self.canvas_width = Some(canvas_size.x);

        // draw the outer border
        self.gam.draw_rounded_rectangle(self.canvas,
            RoundedRectangle::new(
                Rectangle::new_with_style(Point::new(0, 0), canvas_size,
                    DrawStyle::new(PixelColor::Light, PixelColor::Dark, 3)
                ), 5
            )).unwrap();

        // draw the line items
        // we require that the items list be in index-order, with no holes: we abort at the first None item
        let mut cur_index: i16 = 0;
        for maybe_item in self.items.iter() {
            if let Some(_item) = maybe_item {
                if self.index == cur_index as usize {
                    self.draw_item(cur_index as i16, true);
                } else {
                    self.draw_item(cur_index as i16, false);
                }
                if cur_index != 0 {
                    self.draw_divider(cur_index);
                }

                cur_index += 1;
            } else {
                break;
            }
        }
        self.gam.redraw().unwrap();
    }
    fn num_items(&self) -> usize {
        let mut items = 0;
        for maybe_item in self.items.iter() {
            if maybe_item.is_some() {
                items += 1;
            } else {
                break;
            }
        }
        items
    }
    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::debug!("got key '{}'", k);
            match k {
                '∴' => {
                    if let Some(mi) = self.items[self.index] {
                        // give up focus before issuing the command, as some commands conflict with loss of focus...
                        if mi.close_on_select {
                            self.gam.relinquish_focus().unwrap();
                        }
                        log::debug!("doing menu action for {}", mi.name);
                        match mi.action_payload {
                            MenuPayload::Scalar(args) => {
                                xous::send_message(mi.action_conn,
                                    xous::Message::new_scalar(mi.action_opcode as usize,
                                        args[0] as usize, args[1] as usize, args[2] as usize, args[3] as usize)
                                ).expect("couldn't send menu action");
                            },
                            MenuPayload::Memory((_buf, _len)) => {
                                unimplemented!("menu buffer targets are a future feature");
                            }
                        }
                        self.index = 0; // reset the index to 0
                    }
                    self.gam.redraw().unwrap();
                },
                '←' => {
                    // placeholder
                    log::trace!("got left arrow");
                }
                '→' => {
                    // placeholder
                    log::trace!("got right arrow");
                }
                '↑' => {
                    self.prev_item();
                    self.gam.redraw().unwrap();
                }
                '↓' => {
                    self.next_item();
                    self.gam.redraw().unwrap();
                }
                _ => {}
            }
        }
    }
}
