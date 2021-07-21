
/*
  design ideas

    ---------------------
    | Password Type: Updater
    | Requester: RootKeys
    | Reason: The updater modal has not been set.
    | Security Level: Critical
    |
    |    *****4f_
    |
    | [ ] Hide as you type
    | [x] Display last chars
    | [ ] Show as you type
    |
    | [x] Persist until reboot
    | [ ] Persist until suspend
    | [ ] Use once
    ---------------------
*/

use crate::api::*;
use crate::Gam;

use graphics_server::api::{TextOp, TextView};

use graphics_server::api::{Point, Gid, Line, Rectangle, Circle, RoundedRectangle, TokenClaim};
pub use graphics_server::GlyphStyle;
// menu imports
use graphics_server::api::{PixelColor, TextBounds, DrawStyle};

use xous::{send_message, CID, Message};
use xous_ipc::{String, Buffer};
use num_traits::*;

const MAX_ITEMS: usize = 16;

#[allow(dead_code)] // here until Memory types are implemented
#[derive(Debug, Copy, Clone)]
pub enum ModalPayload {
    /// memorized scalar payload
    Scalar([u32; 4]),
    /// this a nebulous-but-TBD maybe way of bodging in a more complicated record, which would involve
    /// casting this memorized, static payload into a Buffer and passing it on. Let's not worry too much about it for now, it's mostly apirational...
    Memory(([u8; 256], usize)),
}
#[derive(Debug, Copy, Clone)]
pub struct ModalItem {
    pub name: String::<64>,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: ModalPayload,
    pub close_on_select: bool,
}

#[derive(Debug)]
pub struct Modal {
    pub sid: xous::SID,
    pub gam: Gam,
    pub xns: xous_names::XousNames,
    pub items: [Option<ModalItem>; MAX_ITEMS],
    pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
    pub divider_margin: i16,
    pub line_height: i16,
    pub canvas_width: Option<i16>,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ModalOpcode {
    Redraw,
    Rawkeys,
    Quit,
}

impl Modal {
    pub fn new(name: &str) -> Modal {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private modal message server");
        let gam = Gam::new(&xns).expect("can't connect to GAM");
        let authtoken = gam.register_ux(
            UxRegistration {
                app_name: String::<128>::from_str(name),
                ux_type: UxType::Modal,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: ModalOpcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                rawkeys_id: Some(ModalOpcode::Rawkeys.to_u32().unwrap()),
            }
        ).expect("couldn't register my Ux element with GAM");
        assert!(authtoken.is_some(), "Couldn't register modal. Did you remember to add the app_name to the tokens.rs expected boot contexts list?");
        log::debug!("requesting content canvas for modal");
        let canvas = gam.request_content_canvas(authtoken.unwrap()).expect("couldn't get my content canvas from GAM");
        let line_height = gam.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height hint") as i16;
        Modal {
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
    // if successful, returns None, otherwise, the modal item
    pub fn add_item(&mut self, new_item: ModalItem) -> Option<ModalItem> {
        // first, do the insertion.
        // add the modal item to the first free slot
        // any modifications to the modal structure should guarantee that the list is compacted
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
                total_items = 1; // just so we see a blank modal at least, and have a clue how to debug
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

            self.gam.post_textview(&mut item_tv).expect("couldn't render modal list item");
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
        log::trace!("modal redraw");
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
                        log::debug!("doing modal action for {}", mi.name);
                        match mi.action_payload {
                            ModalPayload::Scalar(args) => {
                                xous::send_message(mi.action_conn,
                                    xous::Message::new_scalar(mi.action_opcode as usize,
                                        args[0] as usize, args[1] as usize, args[2] as usize, args[3] as usize)
                                ).expect("couldn't send modal action");
                            },
                            ModalPayload::Memory((_buf, _len)) => {
                                unimplemented!("modal buffer targets are a future feature");
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
