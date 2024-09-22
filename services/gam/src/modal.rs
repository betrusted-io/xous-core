//! The main API entry point is the `Modal` struct. Click into the struct for more details.

mod textentry;
pub use textentry::*;
mod radiobuttons;
pub use radiobuttons::*;
mod checkboxes;
pub use checkboxes::*;
mod notification;
pub use notification::*;
mod slider;
pub use slider::*;
mod progressbar;
pub use progressbar::*;
mod consoleinput;
pub use consoleinput::*;
#[cfg(feature = "ditherpunk")]
mod image;
#[cfg(feature = "ditherpunk")]
pub use image::*;
mod bip39entry;
use core::fmt::Write;

pub use bip39entry::*;
use enum_dispatch::enum_dispatch;
pub use graphics_server::api::GlyphStyle;
use graphics_server::api::*;
use num_traits::*;
use xous_ipc::Buffer;

use crate::api::*;
use crate::Gam;
use crate::MsgForwarder;

pub const MAX_ITEMS: usize = 8;

#[enum_dispatch(ActionApi)]
pub enum ActionType {
    TextEntry,
    Bip39Entry,
    RadioButtons,
    CheckBoxes,
    Slider,
    Notification,
    #[cfg(feature = "ditherpunk")]
    Image,
    ConsoleInput,
}

#[enum_dispatch]
pub trait ActionApi {
    fn height(&self, glyph_height: i16, margin: i16, _modal: &Modal) -> i16 { glyph_height + margin * 2 }
    fn redraw(&self, _at_height: i16, _modal: &Modal) { unimplemented!() }
    fn close(&mut self) {}
    fn is_password(&self) -> bool { false }
    /// navigation is one of '∴' | '←' | '→' | '↑' | '↓'
    fn key_action(&mut self, _key: char) -> Option<ValidatorErr> { None }
    fn set_action_opcode(&mut self, _op: u32) {}
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum ModalOpcode {
    // if changes are made here, also update MenuOpcode
    Redraw = 0x4000_0000, /* set the high bit so that "standard" enums don't conflict with the
                           * Modal-specific opcodes */
    Rawkeys,
    Quit,
}

/// We use a new type for item names, so that it's easy to resize this as needed.
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct ItemName(String);
impl ItemName {
    pub fn new(name: &str) -> Self { ItemName(String::from(name)) }

    pub fn as_str(&self) -> &str { self.0.as_str() }
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq, Default)]
pub struct Bip39EntryPayload {
    // up to 32 bytes (256 bits) could be entered
    pub data: [u8; 32],
    // the actual length entered is reported here
    pub len: u32,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayload {
    dirty: bool,
    pub content: String,
    pub placeholder: Option<String>,
    pub placeholder_persist: bool,
    pub insertion_point: Option<usize>,
}

impl TextEntryPayload {
    pub fn new() -> Self {
        TextEntryPayload {
            dirty: Default::default(),
            content: Default::default(),
            placeholder: Default::default(),
            placeholder_persist: false,
            insertion_point: None,
        }
    }

    pub fn new_with_fields(content: String, placeholder: Option<String>) -> Self {
        TextEntryPayload {
            dirty: false,
            content,
            placeholder,
            placeholder_persist: false,
            insertion_point: None,
        }
    }

    /// Ensures that 0's are written to the storage of this struct, and not optimized out; important for
    /// password fields.
    pub fn volatile_clear(&mut self) { self.content.volatile_clear(); }

    pub fn as_str(&self) -> &str { self.content.as_str() }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct SliderPayload(pub u32);
impl SliderPayload {
    pub fn new(value: u32) -> Self { SliderPayload(value) }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RadioButtonPayload(pub ItemName); // returns the name of the item corresponding to the radio button selection
impl RadioButtonPayload {
    pub fn new(name: &str) -> Self { RadioButtonPayload(ItemName::new(name)) }

    pub fn as_str(&self) -> &str { self.0.as_str() }

    pub fn clear(&mut self) { self.0.0.clear(); }
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct CheckBoxPayload(pub [Option<ItemName>; MAX_ITEMS]); // returns a list of potential items that could be selected
impl CheckBoxPayload {
    pub fn new() -> Self { CheckBoxPayload([None; MAX_ITEMS]) }

    pub fn payload(&self) -> [Option<ItemName>; MAX_ITEMS] { self.0 }

    pub fn contains(&self, name: &str) -> bool {
        for maybe_item in self.0.iter() {
            if let Some(item) = maybe_item {
                if item.as_str() == name {
                    return true;
                }
            }
        }
        false
    }

    pub fn add(&mut self, name: &str) -> bool {
        if self.contains(name) {
            return true;
        }
        for maybe_item in self.0.iter_mut() {
            if maybe_item.is_none() {
                *maybe_item = Some(ItemName::new(name));
                return true;
            }
        }
        false
    }

    pub fn remove(&mut self, name: &str) -> bool {
        for maybe_item in self.0.iter_mut() {
            if let Some(item) = maybe_item {
                if item.as_str() == name {
                    *maybe_item = None;
                    return true;
                }
            }
        }
        false
    }
}

//#[derive(Debug)]
pub struct Modal<'a> {
    pub sid: xous::SID,
    pub gam: Gam,
    pub xns: xous_names::XousNames,
    pub top_text: Option<TextView>,
    pub bot_text: Option<TextView>,
    pub action: ActionType,
    /// This is a slightly unsafe option, in that it only works if you have a simple TextEntry
    /// box. It enables Line-by-line text reflow selection. If false, then each entry gets only one line.
    /// If set to true, operations may slow down proportionally with the size of the text as we have to
    /// recompute text reflow every time the object is touched. The algorithm will greedily consume
    /// space in the canvas until the entire canvas is filled with the TextEntry box. This means that
    /// if you have a compound modal (multiple elements in it), the extra elements will be flowed off
    /// the bottom.
    pub growable: bool,

    //pub index: usize, // currently selected item
    pub canvas: Gid,
    pub authtoken: [u32; 4],
    pub margin: i16,
    pub line_height: i16,
    pub canvas_width: i16,
    pub maximal_height: i16,
    pub inverted: bool,
    pub style: GlyphStyle,
    pub helper_data: Option<Buffer<'a>>,
    pub name: String,

    // optimize draw time
    top_dirty: bool,
    top_memoized_height: Option<i16>,
    bot_dirty: bool,
    bot_memoized_height: Option<i16>,
}

fn recompute_canvas(modal: &mut Modal, top_text: Option<&str>, bot_text: Option<&str>, style: GlyphStyle) {
    // we need to set a "max" size to our modal box, so that the text computations don't fail later on
    let current_bounds = modal.gam.get_canvas_bounds(modal.canvas).expect("couldn't get current bounds");

    // method:
    //   - we assume the GAM gives us an initial modal with a "maximum" height setting
    //   - items are populated within this maximal canvas setting, and then the actual height needed is
    //     computed
    //   - the canvas is resized to this actual height
    // problems:
    //   - there is no sanity check on the size of the text boxes. So if you give the UX element a top_text
    //     box that's huge, it will just overflow the canvas size and nothing else will get drawn.

    let mut total_height = modal.margin;
    log::trace!("step 0 total_height: {}", total_height);
    // compute height of top_text, if any
    if let Some(top_str) = top_text {
        let mut top_tv = TextView::new(
            modal.canvas,
            TextBounds::GrowableFromTl(
                Point::new(modal.margin, modal.margin),
                (modal.canvas_width - modal.margin * 2) as u16,
            ),
        );
        top_tv.draw_border = false;
        top_tv.style = style;
        top_tv.margin = Point::new(0, 0); // all margin already accounted for in the raw bounds of the text drawing
        top_tv.ellipsis = false;
        top_tv.invert = modal.inverted;
        // specify a clip rect that's the biggest possible allowed. If we don't do this, the current canvas
        // bounds are used, and the operation will fail if the text has to get bigger.
        top_tv.clip_rect = Some(Rectangle::new(
            Point::new(0, 0),
            Point::new(current_bounds.x, crate::api::MODAL_Y_MAX - 2 * modal.line_height),
        ));
        write!(top_tv.text, "{}", top_str).unwrap();

        log::trace!("posting top tv: {:?}", top_tv);
        modal.gam.bounds_compute_textview(&mut top_tv).expect("couldn't simulate top text size");
        if let Some(bounds) = top_tv.bounds_computed {
            log::trace!("top_tv bounds computed {}", bounds.br.y - bounds.tl.y);
            total_height += bounds.br.y - bounds.tl.y;
        } else {
            log::warn!("couldn't compute height for modal top_text: {:?}", top_tv);
            // probably should find a better way to deal with this.
            total_height += crate::api::MODAL_Y_MAX - (modal.line_height * 2);
        }
        modal.top_text = Some(top_tv);
    }
    total_height += modal.margin;

    // compute height of action item
    log::trace!("step 1 total_height: {}", total_height);
    total_height += modal.action.height(modal.line_height, modal.margin, &modal);
    total_height += modal.margin;

    // compute height of bot_text, if any
    log::trace!("step 2 total_height: {}", total_height);
    if let Some(bot_str) = bot_text {
        let mut bot_tv = TextView::new(
            modal.canvas,
            TextBounds::GrowableFromTl(
                Point::new(modal.margin, total_height),
                (modal.canvas_width - modal.margin * 2) as u16,
            ),
        );
        bot_tv.draw_border = false;
        bot_tv.style = style;
        bot_tv.margin = Point::new(0, 0); // all margin already accounted for in the raw bounds of the text drawing
        bot_tv.ellipsis = false;
        bot_tv.invert = modal.inverted;
        // specify a clip rect that's the biggest possible allowed. If we don't do this, the current canvas
        // bounds are used, and the operation will fail if the text has to get bigger.
        bot_tv.clip_rect = Some(Rectangle::new(
            Point::new(0, 0),
            Point::new(current_bounds.x, crate::api::MODAL_Y_MAX - 2 * modal.line_height),
        ));
        write!(bot_tv.text, "{}", bot_str).unwrap();

        log::trace!("posting bot tv: {:?}", bot_tv);
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
    // don't send the request if there is no change in the size of things. This is because the request is
    // expensive -- it will result in a redraw of everything, plus defacement, etc.
    if new_bounds.requested != current_bounds {
        log::debug!("applying recomputed bounds of {:?}", new_bounds);
        modal.gam.set_canvas_bounds_request(&mut new_bounds).expect("couldn't call set bounds");
    }
}

impl<'a> Modal<'a> {
    pub fn new(
        name: &str,
        action: ActionType,
        top_text: Option<&str>,
        bot_text: Option<&str>,
        style: GlyphStyle,
        margin: i16,
    ) -> Modal<'a> {
        let xns = xous_names::XousNames::new().unwrap();
        let sid = xous::create_server().expect("can't create private modal message server");
        let gam = Gam::new(&xns).expect("can't connect to GAM");
        let authtoken = gam
            .register_ux(UxRegistration {
                app_name: String::from(name),
                ux_type: UxType::Modal,
                predictor: None,
                listener: sid.to_array(),
                redraw_id: ModalOpcode::Redraw.to_u32().unwrap(),
                gotinput_id: None,
                audioframe_id: None,
                focuschange_id: None, // should always be none because we're not an app
                rawkeys_id: Some(ModalOpcode::Rawkeys.to_u32().unwrap()),
            })
            .expect("couldn't register my Ux element with GAM");
        assert!(
            authtoken.is_some(),
            "Couldn't register modal. Did you remember to add the app_name to the tokens.rs expected boot contexts list?"
        );
        log::debug!("requesting content canvas for modal");
        let canvas =
            gam.request_content_canvas(authtoken.unwrap()).expect("couldn't get my content canvas from GAM");
        let line_height = if locales::LANG == "zh" {
            // zh has no "small" style
            gam.glyph_height_hint(GlyphStyle::Regular).expect("couldn't get glyph height hint") as i16
        } else {
            gam.glyph_height_hint(style).expect("couldn't get glyph height hint") as i16
        };
        let canvas_bounds = gam.get_canvas_bounds(canvas).expect("couldn't get starting canvas bounds");

        log::trace!("initializing Modal structure");
        // check to see if this is a password field or not
        // note: if a modal claims it's a password field but lacks sufficient trust level, the GAM will refuse
        // to render the element.
        let inverted = match action {
            ActionType::TextEntry(_) => action.is_password(),
            _ => false,
        };

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
            margin,
            line_height,
            canvas_width: canvas_bounds.x, // memoize this, it shouldn't change
            maximal_height: 402,           /* arbitrary number set for aesthetic reasons; limits growth of
                                            * modals that request reflowable/growable text boxes */
            inverted,
            style,
            helper_data: None,
            name: String::from(name),
            top_dirty: true,
            bot_dirty: true,
            top_memoized_height: None,
            bot_memoized_height: None,
            growable: false,
        };
        recompute_canvas(&mut modal, top_text, bot_text, style);
        modal
    }

    pub fn activate(&self) {
        const POLL_DELAY_MS: usize = 857;
        match self.gam.raise_modal(self.name.to_str()) {
            Ok(_) => (),
            Err(_) => {
                std::thread::spawn({
                    let name = self.name.clone();
                    move || {
                        let xns = xous_names::XousNames::new().unwrap();
                        let ticktimer = ticktimer_server::Ticktimer::new().unwrap();
                        ticktimer.sleep_ms(POLL_DELAY_MS).unwrap();
                        let gam = crate::Gam::new(&xns).unwrap();
                        while gam.raise_modal(name.to_str()).is_err() {
                            log::info!("Couldn't raise {}; retrying...", name);
                            ticktimer.sleep_ms(POLL_DELAY_MS).unwrap();
                        }
                        log::info!("Success! exiting polling thread.");
                    }
                });
            }
        }
    }

    /// this function spawns a client-side thread to forward redraw and key event
    /// messages on to a local server. The goal is to keep the local server's SID
    /// a secret. The GAM only knows the single-use SID for redraw commands; this
    /// isolates a server's private command set from the GAM.
    pub fn spawn_helper(
        &mut self,
        private_sid: xous::SID,
        public_sid: xous::SID,
        redraw_op: u32,
        rawkeys_op: u32,
        drop_op: u32,
    ) {
        let helper_data = MsgForwarder {
            private_sid: private_sid.to_array(),
            public_sid: public_sid.to_array(),
            redraw_op,
            rawkeys_op,
            drop_op,
        };
        let buf = Buffer::into_buf(helper_data).expect("couldn't allocate helper data for helper thread");
        let (addr, size, offset) = unsafe { buf.to_raw_parts() };
        self.helper_data = Some(buf);
        xous::create_thread_3(crate::forwarding_thread, addr, size, offset)
            .expect("couldn't spawn a helper thread");
    }

    pub fn redraw(&mut self) {
        const BORDER_WIDTH: i16 = 3;
        log::debug!("modal redraw");
        let canvas_size = self.gam.get_canvas_bounds(self.canvas).unwrap();
        let do_redraw = self.top_dirty || self.bot_dirty || self.inverted;
        // draw the outer border
        if do_redraw {
            self.gam
                .draw_rounded_rectangle(
                    self.canvas,
                    RoundedRectangle::new(
                        Rectangle::new_with_style(
                            Point::new(0, 0),
                            canvas_size,
                            DrawStyle::new(
                                if self.inverted { PixelColor::Dark } else { PixelColor::Light },
                                PixelColor::Dark,
                                BORDER_WIDTH,
                            ),
                        ),
                        5,
                    ),
                )
                .unwrap();
        }

        let mut cur_height = self.margin;
        if let Some(mut tv) = self.top_text {
            if do_redraw {
                self.gam.post_textview(&mut tv).expect("couldn't draw text");
                if let Some(bounds) = tv.bounds_computed {
                    let y = bounds.br.y - bounds.tl.y;
                    let y_clip = if y > MODAL_Y_MAX - self.line_height * 3 {
                        log::warn!("overside text, clipping back {}", MODAL_Y_MAX - (self.line_height * 2));
                        MODAL_Y_MAX - (self.line_height * 2)
                    } else {
                        y
                    };
                    cur_height += y_clip;
                    log::trace!("top_tv height: {}", y_clip);
                    self.top_memoized_height = Some(y_clip);
                } else {
                    log::warn!("text bounds didn't compute setting to max");
                    self.top_memoized_height = Some(MODAL_Y_MAX - (self.line_height * 2));
                }
                self.top_dirty = false;
            } else {
                cur_height +=
                    self.top_memoized_height.expect("internal error: memoization didn't work correctly");
            }
        } else {
            self.top_dirty = false;
        }

        let action_height = self.action.height(self.line_height, self.margin, &self);
        if !do_redraw {
            // the action area wasn't blanked, so blank it as prep for the action redraw
            self.gam
                .draw_rectangle(
                    self.canvas,
                    Rectangle::new_with_style(
                        Point::new(BORDER_WIDTH, cur_height),
                        Point::new(canvas_size.x - BORDER_WIDTH, cur_height + action_height),
                        DrawStyle::new(
                            if self.inverted { PixelColor::Dark } else { PixelColor::Light },
                            if self.inverted { PixelColor::Dark } else { PixelColor::Light },
                            0,
                        ),
                    ),
                )
                .unwrap();
        }
        self.action.redraw(cur_height, &self);
        cur_height += action_height;

        if let Some(mut tv) = self.bot_text {
            if do_redraw {
                self.gam.post_textview(&mut tv).expect("couldn't draw text");
                if let Some(bounds) = tv.bounds_computed {
                    cur_height += bounds.br.y - bounds.tl.y;
                    self.bot_memoized_height = Some(bounds.br.y - bounds.tl.y);
                }
                self.bot_dirty = false;
            } else {
                cur_height +=
                    self.bot_memoized_height.expect("internal error: memoization didn't work correctly");
            }
        } else {
            self.bot_dirty = false;
        }
        log::trace!("total height: {}", cur_height);
        log::trace!("modal redraw##");
        self.gam.redraw().unwrap();
    }

    pub fn key_event(&mut self, keys: [char; 4]) {
        for &k in keys.iter() {
            if k != '\u{0}' {
                log::debug!("got key '{}'", k);
                let err = self.action.key_action(k);
                if let Some(err_msg) = err {
                    self.modify(None, None, false, Some(err_msg.to_str()), false, None);
                }
            }
        }
        self.redraw();
    }

    /// This empowers an action within a modal to potentially consume all the available height in a canvas
    /// The current implementation works if you have a "simple" TextEntry box, but it will fail if you have
    /// stuff below it because the algorithm can't "see" the reserved space at the moment for extra items
    /// below.
    pub fn set_growable(&mut self, state: bool) { self.growable = state; }

    /// this function will modify UX elements if any of the arguments are Some()
    /// if None, the element is unchanged.
    /// If a text section is set to remove, but Some() is given for the update, the text is not removed, and
    /// instead replaced with the updated text.
    pub fn modify(
        &mut self,
        update_action: Option<ActionType>,
        update_top_text: Option<&str>,
        remove_top: bool,
        update_bot_text: Option<&str>,
        remove_bot: bool,
        update_style: Option<GlyphStyle>,
    ) {
        if let Some(action) = update_action {
            self.action = action;
        };

        if remove_top {
            self.top_dirty = true;
            self.top_text = None;
        }
        if remove_bot {
            self.bot_dirty = true;
            self.bot_text = None;
        }
        if update_top_text.is_some() {
            self.top_dirty = true;
        }
        if update_bot_text.is_some() {
            self.bot_dirty = true;
        }

        let mut top_tv_temp = String::new(); // size matches that used in TextView
        if let Some(top_text) = update_top_text {
            write!(top_tv_temp, "{}", top_text).unwrap();
        } else {
            if let Some(top_text) = self.top_text {
                write!(top_tv_temp, "{}", top_text).unwrap();
            }
        };
        let top_text = if self.top_text.is_none() && update_top_text.is_none() {
            None
        } else {
            Some(top_tv_temp.to_str())
        };

        let mut bot_tv_temp = String::new(); // size matches that used in TextView
        if let Some(bot_text) = update_bot_text {
            write!(bot_tv_temp, "{}", bot_text).unwrap();
        } else {
            if let Some(bot_text) = self.bot_text {
                write!(bot_tv_temp, "{}", bot_text).unwrap();
            }
        };
        let bot_text = if self.bot_text.is_none() && update_bot_text.is_none() {
            None
        } else {
            Some(bot_tv_temp.to_str())
        };

        let style = if let Some(style) = update_style {
            self.top_dirty = true;
            self.bot_dirty = true;
            style
        } else {
            self.style
        };
        recompute_canvas(self, top_text, bot_text, style);
    }
}

/*
 old notes to remind myself of how I got here.

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
