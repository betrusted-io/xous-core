
/*
  design ideas

Modal for password request:
    ---------------------
    | Password Type: Updater
    | Requester: RootKeys
    | Reason: The updater modal has not been set.
    | Security Level: Critical
    |
    |    *****4f_
    |
    |      ← 👁️ 🕶️ * →
    |--------------------

Item primitives:
  - text bubble
  - text entry field (with confidentiality option)
  - left/right radio select
  - up/down radio select

Then simple menu prompt after password entry:
    ---------------------
    | [x] Persist until reboot
    | [ ] Persist until suspend
    | [ ] Use once
    ---------------------

General form for modals:

    [top text]

    [action form]

    [bottom text]

 - "top text" is an optional TextArea
 - "action form" is a mandatory field that handles interactions
 - "bottom text" is an optional TextArea

 Action form can be exactly one of the following:
   - password text field - enter closes the form, has visibility options as left/right arrows; entered text wraps
   - regular text field - enter closes the form, visibility is always visible; entered text wraps
   - radio buttons - has an explicit "okay" button to close the modal; up/down arrows + select/enter pick the radio
   - check boxes - has an explicit "okay" button to close the modal; up/down arrows + select/enter checks boxes
   - slider - left/right moves the slider, enter/select closes the modal
*/
use enum_dispatch::enum_dispatch;

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

const MAX_ITEMS: usize = 8;

#[derive(Debug, Copy, Clone)]
pub struct ItemName(String::<64>);
#[derive(Debug, Copy, Clone)]
pub struct TextEntryPayload(String::<256>);
#[derive(Debug, Copy, Clone)]
pub struct RadioButtonPayload(ItemName); // returns the name of the item corresponding to the radio button selection
#[derive(Debug, Copy, Clone)]
pub struct CheckBoxPayload([Option<ItemName>; MAX_ITEMS]); // returns a list of potential items that could be selected

#[derive(Debug, Copy, Clone)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars,
    /// all chars hidden as *
    Hidden,
}
#[derive(Debug, Copy, Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: Option<TextEntryPayload>,
}
impl ActionApi for TextEntry {
    fn height(&self) -> i16 {
        16  // placeholder
    }
}
#[derive(Debug, Copy, Clone)]
pub struct RadioButtons {
    pub items: [Option<ItemName>; MAX_ITEMS],
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: Option<RadioButtonPayload>,
}
impl ActionApi for RadioButtons {
    fn height(&self) -> i16 {
        16  // placeholder
    }
}
#[derive(Debug, Copy, Clone)]
pub struct CheckBoxes {
    pub items: [Option<ItemName>; MAX_ITEMS],
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: Option<CheckBoxPayload>,
}
impl ActionApi for CheckBoxes {
    fn height(&self) -> i16 {
        16  // placeholder
    }
}
#[derive(Debug, Copy, Clone)]
pub struct Slider {
    pub min: u32,
    pub max: u32,
    pub step: u32,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: u32,
}
impl ActionApi for Slider {
    fn height(&self) -> i16 {
        16  // placeholder
    }
}

#[enum_dispatch]
trait ActionApi {
    fn height(&self) -> i16 {16} // get the computed height for an action
}

#[enum_dispatch(ActionApi)]
#[derive(Debug, Copy, Clone)]
pub enum ActionType {
    TextEntry,
    RadioButtons,
    CheckBoxes,
    Slider
}

#[derive(Debug)]
pub struct Modal {
    pub sid: xous::SID,
    pub gam: Gam,
    pub xns: xous_names::XousNames,
    pub top_text: Option<TextView>,
    pub bot_text: Option<TextView>,
    pub action: ActionType,

    //pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
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
    pub fn new(name: &str, action: ActionType, top_text: Option<TextView>, bot_text: Option<TextView>) -> Modal {
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

        // we now have a canvas that is some minimal height, but with the final width as allowed by the GAM.
        // compute the final height based upon the contents within.
        let mut modal = Modal {
            sid,
            gam,
            xns,
            top_text,
            bot_text,
            action,
            canvas,
            authtoken: authtoken.unwrap(),
            margin: 4,
            line_height,
            canvas_width: None,
        };

        let mut total_height = modal.margin * 2;
        // compute height of top_text, if any
        if let Some(mut text) = modal.top_text {
            text.dry_run = true;
            modal.gam.post_textview(&mut text).expect("couldn't simulate top text size");
            if let Some(bounds) = text.bounds_computed {
                total_height += bounds.br.y - bounds.tl.y;
            } else {
                log::error!("couldn't compute height for modal top_text: {:?}", text);
                panic!("couldn't compute height for modal top_text");
            }
        }

        // compute height of action item
        total_height += modal.action.height();

        // compute height of bot_text, if any
        if let Some(mut text) = modal.bot_text {
            text.dry_run = true;
            modal.gam.post_textview(&mut text).expect("couldn't simulate bot text size");
            if let Some(bounds) = text.bounds_computed {
                total_height += bounds.br.y - bounds.tl.y;
            } else {
                log::error!("couldn't compute height for modal bot_text: {:?}", text);
                panic!("couldn't compute height for modal bot_text");
            }
        }

        let current_bounds = modal.gam.get_canvas_bounds(modal.canvas).expect("couldn't get current bounds");
        let mut new_bounds = SetCanvasBoundsRequest {
            requested: Point::new(current_bounds.x, total_height),
            granted: None,
            token_type: TokenType::App,
            token: modal.authtoken,
        };
        log::debug!("modal requesting bounds of {:?}", new_bounds);
        modal.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");

        modal
    }

    /*
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
    */

    pub fn redraw(&mut self) {
        // for now, just draw a black rectangle
        log::debug!("modal redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        self.canvas_width = Some(canvas_size.x);

        // draw the outer border
        self.gam.draw_rounded_rectangle(self.canvas,
            RoundedRectangle::new(
                Rectangle::new_with_style(Point::new(0, 0), canvas_size,
                    DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 3)
                ), 5
            )).unwrap();
        /*
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
        */
        self.gam.redraw().unwrap();
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            log::debug!("got key '{}'", k);
            match k {
                '∴' => {
                        /*
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
                    */
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
                    //self.prev_item();
                    self.gam.redraw().unwrap();
                }
                '↓' => {
                    //self.next_item();
                    self.gam.redraw().unwrap();
                }
                _ => {}
            }
        }
    }
}
