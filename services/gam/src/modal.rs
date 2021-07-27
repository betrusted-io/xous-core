
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

use core::fmt::Write;

const MAX_ITEMS: usize = 8;

#[derive(Debug, Copy, Clone)]
pub struct ItemName(String::<64>);
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq)]
pub struct TextEntryPayload(String::<256>);
impl TextEntryPayload {
    pub fn new() -> Self {
        TextEntryPayload(String::<256>::new())
    }
    pub fn volatile_clear(&mut self) {
        self.0.volatile_clear(); // volatile_clear() ensures that 0's are written and not optimized out; important for password fields
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str().expect("couldn't convert password string")
    }
}

#[derive(Debug, Copy, Clone)]
pub struct RadioButtonPayload(ItemName); // returns the name of the item corresponding to the radio button selection
#[derive(Debug, Copy, Clone)]
pub struct CheckBoxPayload([Option<ItemName>; MAX_ITEMS]); // returns a list of potential items that could be selected

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible = 0,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars = 1,
    /// all chars hidden as *
    Hidden = 2,
}
#[derive(Debug, Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: TextEntryPayload,
}
impl ActionApi for TextEntry {
    fn is_password(&self) -> bool {
        self.is_password
    }
    /// The total canvas height is computed with this API call
    /// The canvas height is not dynamically adjustable for modals.
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {
        /*
            -------------------
            | ****            |    <-- glyph_height + 2*margin
            -------------------
                ← 👁️ 🕶️ * →        <-- glyph_height

            + 2 * margin top/bottom

            auto-closes on enter
        */
        glyph_height + 2*margin + glyph_height + 2*margin + 8 // 8 pixels extra margin because the emoji glyphs are oversized
    }
    fn redraw(&self, at_height: i16, modal: &Modal) {
        let color = if self.is_password {
            PixelColor::Light
        } else {
            PixelColor::Dark
        };

        // draw the currently entered text
        let mut tv = TextView::new(
            modal.canvas,
            TextBounds::BoundingBox(Rectangle::new(
                Point::new(modal.margin, at_height),
                Point::new(modal.canvas_width - modal.margin, at_height + modal.line_height))
        ));
        tv.ellipsis = true; // TODO: fix so we are drawing from the right-most entered text and old text is ellipsis *to the left*
        tv.invert = self.is_password;
        tv.style = modal.style;
        tv.margin = Point::new(0, 0);
        tv.draw_border = false;
        tv.insertion = Some(self.action_payload.0.len() as i32);
        tv.text.clear(); // make sure this is blank
        let payload_chars = self.action_payload.0.as_str().unwrap().chars().count();
        // TODO: condense the "above 20" chars length path a bit -- written out "the dumb way" just to reason out the logic a bit
        match self.visibility {
            TextEntryVisibility::Visible => {
                log::trace!("action payload: {}", self.action_payload.0.as_str().unwrap());
                if payload_chars < 20 {
                    write!(tv.text, "{}", self.action_payload.0.as_str().unwrap());
                } else {
                    write!(tv.text, "...{}", &self.action_payload.0.as_str().unwrap()[payload_chars-18..]);
                }
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            },
            TextEntryVisibility::Hidden => {
                if payload_chars < 20 {
                    for _char in self.action_payload.0.as_str().unwrap().chars() {
                        tv.text.push('*').expect("text field too long");
                    }
                } else {
                    // just render a pure dummy string
                    tv.text.push('.').unwrap();
                    tv.text.push('.').unwrap();
                    tv.text.push('.').unwrap();
                    for _ in 0..18 {
                        tv.text.push('*').expect("text field too long");
                    }
                }
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            },
            TextEntryVisibility::LastChars => {
                if payload_chars < 20 {
                    let hide_to = if self.action_payload.0.as_str().unwrap().chars().count() >= 2 {
                        self.action_payload.0.as_str().unwrap().chars().count() - 2
                    } else {
                        0
                    };
                    for (index, ch) in self.action_payload.0.as_str().unwrap().chars().enumerate() {
                        if index < hide_to {
                            tv.text.push('*').expect("text field too long");
                        } else {
                            tv.text.push(ch).expect("text field too long");
                        }
                    }
                } else {
                    tv.text.push('.').unwrap();
                    tv.text.push('.').unwrap();
                    tv.text.push('.').unwrap();
                    let hide_to = if self.action_payload.0.as_str().unwrap().chars().count() >= 2 {
                        self.action_payload.0.as_str().unwrap().chars().count() - 2
                    } else {
                        0
                    };
                    for (index, ch) in self.action_payload.0.as_str().unwrap()[payload_chars-18..].chars().enumerate() {
                        if index + payload_chars-18 < hide_to {
                            tv.text.push('*').expect("text field too long");
                        } else {
                            tv.text.push(ch).expect("text field too long");
                        }
                    }
                }
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            }
        }
        // draw the visibility selection area
        // "<👀🤫✴️>" coded explicitly. Pasting unicode into vscode yields extra cruft that we can't parse (e.g. skin tones and color mods).
        let prompt = "\u{2b05} \u{1f440}\u{1f576}\u{26d4} \u{27a1}";
        let select_index = match self.visibility {
            TextEntryVisibility::Visible => 2,
            TextEntryVisibility::LastChars => 3,
            TextEntryVisibility::Hidden => 4,
        };
        let spacing = 38; // fixed width spacing for the array
        let emoji_width = 36;
        // center the prompt nicely, if possible
        let left_edge = if modal.canvas_width > prompt.chars().count() as i16 * spacing {
            (modal.canvas_width - prompt.chars().count() as i16 * spacing) / 2
        } else {
            0
        };
        for (i, ch) in prompt.chars().enumerate() {
            let mut tv = TextView::new(
                modal.canvas,
                TextBounds::BoundingBox(Rectangle::new(
                    Point::new(left_edge + i as i16 * spacing, at_height + modal.line_height + modal.margin * 4),
                    Point::new(left_edge + i as i16 * spacing + emoji_width, at_height + modal.line_height + 34 + modal.margin * 4))
            ));
            tv.style = GlyphStyle::Regular;
            tv.margin = Point::new(0, 0);
            tv.draw_border = false;
            if i == select_index {
                tv.invert = !self.is_password;
            } else {
                tv.invert = self.is_password;
            }
            tv.text.clear();
            write!(tv.text, "{}", ch).unwrap();
            log::trace!("tv.text: {} : {}/{}", i, tv.text, ch);
            modal.gam.post_textview(&mut tv).expect("couldn't post textview");
        }

        // draw a line for where text gets entered (don't use a box, fitting could be awkward)
        modal.gam.draw_line(modal.canvas, Line::new_with_style(
            Point::new(modal.margin*2, at_height + modal.line_height + modal.margin * 2),
            Point::new(modal.canvas_width - modal.margin*2, at_height + modal.line_height + modal.margin * 2),
            DrawStyle::new(color, color, 1))
            ).expect("couldn't draw entry line");
    }
    fn key_action(&mut self, k: char) {
        log::trace!("key_action: {}", k);
        match k {
            '←' => {
                if self.visibility as u32 > 0 {
                    match FromPrimitive::from_u32(self.visibility as u32 - 1) {
                        Some(new_visibility) => {
                            log::trace!("new visibility: {:?}", new_visibility);
                            self.visibility = new_visibility;
                        },
                        _ => {
                            panic!("internal error: an TextEntryVisibility did not resolve correctly");
                        }
                    }
                }
            },
            '→' => {
                if (self.visibility as u32) < (TextEntryVisibility::Hidden as u32) {
                    match FromPrimitive::from_u32(self.visibility as u32 + 1) {
                        Some(new_visibility) => {
                            log::trace!("new visibility: {:?}", new_visibility);
                            self.visibility = new_visibility
                        },
                        _ => {
                            panic!("internal error: an TextEntryVisibility did not resolve correctly");
                        }
                    }
                }
            },
            '∴' | '\u{d}' => {
                let mut buf = Buffer::into_buf(self.action_payload).expect("couldn't convert message to payload");
                buf.lend(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");
                self.action_payload.volatile_clear(); // ensure the local copy of text is zero'd out
                buf.volatile_clear(); // ensure that the copy of the text used to send the message is also zero'd out
            }
            '↑' | '↓' => {
                // ignore these navigation keys
            }
            '\u{0}' => {
                // ignore null messages
            }
            '\u{8}' => { // backspace
                // coded in a conservative manner to avoid temporary allocations that can leave the plaintext on the stack
                let mut temp_str = String::<256>::from_str(self.action_payload.0.as_str().unwrap());
                let cur_len = temp_str.as_str().unwrap().chars().count();
                let mut c_iter = temp_str.as_str().unwrap().chars();
                self.action_payload.0.clear();
                for _ in 0..cur_len-1 {
                    self.action_payload.0.push(c_iter.next().unwrap());
                }
                temp_str.volatile_clear();
            }
            _ => { // text entry
                self.action_payload.0.push(k).expect("ran out of space storing password");
                log::trace!("****update payload: {}", self.action_payload.0);
            }
        }
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
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {
        let mut total_items = 0;
        // total items, then +1 for the "Okay" message
        for item in self.items.iter().map(|i| if i.is_some(){ total_items += 1} ) {}
        (total_items + 1) * glyph_height + margin * 2
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
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {
        let mut total_items = 0;
        // total items, then +1 for the "Okay" message
        let mut total_items = 0;
        for item in self.items.iter().map(|i| if i.is_some(){ total_items += 1} ) {}
        (total_items + 1) * glyph_height + margin * 2
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
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {
        /*
            min            max    <- glyph height
             -----O----------     <- glyph height
                 [ Okay ]         <- glyph height
        */
        glyph_height * 3 + margin * 2
    }
}





#[enum_dispatch]
trait ActionApi {
    fn height(&self, glyph_height: i16, margin: i16) -> i16 {glyph_height + margin * 2}
    fn redraw(&self, at_height: i16, modal: &Modal) { unimplemented!() }
    fn close(&mut self) {}
    fn is_password(&self) -> bool { false }
    /// navigation is one of '∴' | '←' | '→' | '↑' | '↓'
    fn key_action(&mut self, key: char) {}
}

#[enum_dispatch(ActionApi)]
#[derive(Debug, Clone)]
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
    pub canvas_width: i16,
    pub inverted: bool,
    pub style: GlyphStyle,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ModalOpcode {
    Redraw,
    Rawkeys,
    Quit,
}

impl Modal {
    pub fn new(name: &str, action: ActionType, top_text: Option<&str>, bot_text: Option<&str>, style: GlyphStyle) -> Modal {
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
        let line_height = gam.glyph_height_hint(style).expect("couldn't get glyph height hint") as i16;
        let canvas_bounds = gam.get_canvas_bounds(canvas).expect("couldn't get starting canvas bounds");

        // check to see if this is a password field or not
        // note: if a modal claims it's a password field but lacks sufficient trust level, the GAM will refuse
        // to render the element.
        let inverted = match action {
            ActionType::TextEntry(_) => action.is_password(),
            _ => false
        };

        log::trace!("initializing Modal structure");
        // we now have a canvas that is some minimal height, but with the final width as allowed by the GAM.
        // compute the final height based upon the contents within.
        let mut modal = Modal {
            sid,
            gam,
            xns,
            top_text: None,
            bot_text: None,
            action,
            canvas,
            authtoken: authtoken.unwrap(),
            margin: 4,
            line_height,
            canvas_width: canvas_bounds.x, // memoize this, it shouldn't change
            inverted,
            style,
        };

        // method:
        //   - we assume the GAM gives us an initial modal with a "maximum" height setting
        //   - items are populated within this maximal canvas setting, and then the actual height needed is computed
        //   - the canvas is resized to this actual height
        // problems:
        //   - there is no sanity check on the size of the text boxes. So if you give the UX element a top_text box that's
        //     huge, it will just overflow the canvas size and nothing else will get drawn.

        let mut total_height = modal.margin;
        log::trace!("step 0 total_height: {}", total_height);
        // compute height of top_text, if any
        if let Some(top_str) = top_text {
            let mut top_tv = TextView::new(canvas,
                TextBounds::GrowableFromTl(
                    Point::new(modal.margin, total_height),
                    (modal.canvas_width - modal.margin * 2) as u16
                ));
            top_tv.draw_border = false;
            top_tv.style = style;
            top_tv.margin = Point::new(0, 0,); // all margin already accounted for in the raw bounds of the text drawing
            top_tv.ellipsis = false;
            top_tv.invert = inverted;
            write!(top_tv.text, "{}", top_str);

            log::trace!("posting top tv: {:?}", top_tv);
            modal.gam.bounds_compute_textview(&mut top_tv).expect("couldn't simulate top text size");
            if let Some(bounds) = top_tv.bounds_computed {
                total_height += bounds.br.y - bounds.tl.y;
            } else {
                log::error!("couldn't compute height for modal top_text: {:?}", top_tv);
                panic!("couldn't compute height for modal top_text");
            }
            modal.top_text = Some(top_tv);
        }
        total_height += modal.margin;

        // compute height of action item
        log::trace!("step 1 total_height: {}", total_height);
        total_height += modal.action.height(modal.line_height, modal.margin);
        total_height += modal.margin;

        // compute height of bot_text, if any
        log::trace!("step 2 total_height: {}", total_height);
        if let Some(bot_str) = bot_text {
            let mut bot_tv = TextView::new(canvas,
                TextBounds::GrowableFromTl(
                    Point::new(modal.margin, total_height),
                    (modal.canvas_width - modal.margin * 2) as u16
                ));
            bot_tv.draw_border = false;
            bot_tv.style = style;
            bot_tv.margin = Point::new(0, 0,); // all margin already accounted for in the raw bounds of the text drawing
            bot_tv.ellipsis = false;
            bot_tv.invert = inverted;
            write!(bot_tv.text, "{}", bot_str);

            modal.gam.bounds_compute_textview(&mut bot_tv).expect("couldn't simulate bot text size");
            if let Some(bounds) = bot_tv.bounds_computed {
                total_height += bounds.br.y - bounds.tl.y;
            } else {
                log::error!("couldn't compute height for modal bot_text: {:?}", bot_tv);
                panic!("couldn't compute height for modal bot_text");
            }
            modal.bot_text = Some(bot_tv);
            total_height += modal.margin;
        }
        log::trace!("step 3 total_height: {}", total_height);

        let current_bounds = modal.gam.get_canvas_bounds(modal.canvas).expect("couldn't get current bounds");
        let mut new_bounds = SetCanvasBoundsRequest {
            requested: Point::new(current_bounds.x, total_height),
            granted: None,
            token_type: TokenType::App,
            token: modal.authtoken,
        };
        log::debug!("applying recomputed bounds of {:?}", new_bounds);
        modal.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");

        modal
    }

    pub fn redraw(&self) {
        log::debug!("modal redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        // draw the outer border
        self.gam.draw_rounded_rectangle(self.canvas,
            RoundedRectangle::new(
                Rectangle::new_with_style(Point::new(0, 0), canvas_size,
                    DrawStyle::new(if self.inverted{PixelColor::Dark} else {PixelColor::Light}, PixelColor::Dark, 3)
                ), 5
            )).unwrap();

        let mut cur_height = self.margin;
        if let Some(mut tv) = self.top_text {
            self.gam.post_textview(&mut tv).expect("couldn't draw text");
            if let Some(bounds) = tv.bounds_computed {
                cur_height += bounds.br.y - bounds.tl.y;
            }
        }

        self.action.redraw(cur_height, &self);
        cur_height += self.action.height(self.line_height, self.margin);

        if let Some(mut tv) = self.bot_text {
            self.gam.post_textview(&mut tv).expect("couldn't draw text");
            if let Some(bounds) = tv.bounds_computed {
                cur_height += bounds.br.y - bounds.tl.y;
            }
        }
        log::trace!("total height: {}", cur_height);
        self.gam.redraw().unwrap();
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            if k != '\u{0}' {
                log::debug!("got key '{}'", k);
                self.action.key_action(k); // this is where the action is at
                match k {
                    '∴' | '\u{d}' => {
                        log::trace!("closing modal");
                        // if it's a "close" button, invoke the GAM to put our box away
                        self.gam.relinquish_focus().unwrap();
                    }
                    _ => {
                        // already handled by key_action() above
                    }
                }
            }
        }
        self.redraw();
    }
}
