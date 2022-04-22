use graphics_server::api::GlyphStyle;
use crate::*;
use graphics_server::api::*;

use xous_ipc::{String, Buffer};
use num_traits::*;

use core::fmt::Write;
use core::cell::Cell;

// TODO: figure out this, do we really have to limit ourselves to 10?
const MAX_FIELDS: i16 = 10;

pub type ValidatorErr = xous_ipc::String::<256>;

pub type Payloads = [TextEntryPayload; MAX_FIELDS as usize];

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayloads (Payloads);

impl TextEntryPayloads {
    pub fn first(&self) -> TextEntryPayload {
        self.0[0]
    }

    pub fn content(&self) -> Vec<TextEntryPayload> {
        self.0
        .iter().cloned()
        .filter(|payload| payload.dirty)
        .collect()
    }
}

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible = 0,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars = 1,
    /// all chars hidden as *
    Hidden = 2,
}

#[derive(Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    // validator borrows the text entry payload, and returns an error message if something didn't go well.
    // validator takes as ragument the current action_payload, and the current action_opcode
    pub validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    pub action_payloads: Vec<TextEntryPayload>,

    max_field_amount: u32,
    selected_field: i16,
    field_height: Cell::<i16>,
}

impl Default for TextEntry {
    fn default() -> Self {
        Self {
            is_password: Default::default(),
            visibility: TextEntryVisibility::Visible,
            action_conn: Default::default(),
            action_opcode: Default::default(),
            validator: Default::default(),
            selected_field: Default::default(),
            action_payloads: Default::default(),
            max_field_amount: 0,
            field_height: Cell::new(0),
        }
    }
}

impl TextEntry {
    pub fn new(
        is_password: bool,
        visibility: TextEntryVisibility,
        action_conn: xous::CID,
        action_opcode: u32,
        action_payloads: Vec<TextEntryPayload>,
        validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    ) -> Self {
        if action_payloads.len() as i16 > MAX_FIELDS {
            panic!("can't have more than {} fields, found {}", MAX_FIELDS, action_payloads.len());
        }

        Self {
            is_password,
            visibility,
            action_conn,
            action_opcode,
            action_payloads,
            validator,
            ..Default::default()
        }
    }

    pub fn reset_action_payloads(&mut self, fields: u32, placeholders: Option<[Option<xous_ipc::String<256>>; 10]>) {
        let mut payload = vec![TextEntryPayload::default(); fields as usize];

        if let Some(placeholders) = placeholders {
            for (index, element) in payload.iter_mut().enumerate() {
                element.placeholder = placeholders[index];
            }
        }

        self.action_payloads = payload;
        self.max_field_amount = fields;
    }
}


impl ActionApi for TextEntry {
    fn set_action_opcode(&mut self, op: u32) {self.action_opcode = op}
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
        // the glyph_height is an opaque value because the final value depends upon a lookup table
        // stored in the graphics_server crate. To obtain this would require a handle to that server
        // which is private to the GAM. Thus we receive a copy of this from our caller and stash it
        // here for future reference. `glyph_height` can change depending upon the locale; in particular,
        // CJK languages have taller glyphs.
        self.field_height.set(glyph_height + 2*margin); // stash a copy for later

        // compute the overall_height of the entry fields
        let mut overall_height =
            self.field_height.get() * self.action_payloads.len() as i16;

        // if we're a password, we add an extra glyph_height to the bottom for the text visibility items
        if self.is_password {
            overall_height += glyph_height;
        }

        overall_height
    }
    fn redraw(&self, at_height: i16, modal: &Modal) {
        const MAX_CHARS: usize = 33;
        let color = if self.is_password {
            PixelColor::Light
        } else {
            PixelColor::Dark
        };

        let mut current_height = at_height;
        let payloads = self.action_payloads.clone();

        let bullet_margin = if payloads.len() > 1 {
            17 // this is the margin for drawing the selection bullet
        } else {
            0 // no selection bullet
        };

        for (index, payload) in payloads.iter().enumerate() {
            if index as i16 == self.selected_field && payloads.len() > 1 {
                // draw the dot
                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::BoundingBox(Rectangle::new(
                        Point::new(modal.margin, current_height),
                        Point::new(modal.canvas_width - modal.margin, current_height + modal.line_height))
                ));

                tv.text.clear();
                tv.bounds_computed = None;
                tv.draw_border = false;
                write!(tv, "•").unwrap(); // emoji glyph will be summoned in this case
                modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            }


            let left_text_margin = modal.margin + bullet_margin; // space for the bullet point on the left, if it's there

            // draw the currently entered text
            let mut tv = TextView::new(
                modal.canvas,
                TextBounds::BoundingBox(Rectangle::new(
                    Point::new(left_text_margin, current_height),
                    Point::new(modal.canvas_width - (modal.margin + bullet_margin), current_height + modal.line_height))
            ));
            tv.ellipsis = true;
            tv.invert = self.is_password;
            tv.style = if self.is_password {
                GlyphStyle::Monospace
            } else {
                if payload.placeholder.is_some() && payload.content.len().is_zero() {
                    // note: this is just a "recommendation" - if there is an emoji or chinese character in this string, the height revers to modal.style's height
                    GlyphStyle::Small
                } else {
                    modal.style
                }
            };
            tv.margin = Point::new(0, 0);
            tv.draw_border = false;
            tv.insertion = Some(payload.content.len() as i32);
            tv.text.clear(); // make sure this is blank
            let payload_chars = payload.content.as_str().unwrap().chars().count();
            // TODO: condense the "above MAX_CHARS" chars length path a bit -- written out "the dumb way" just to reason out the logic a bit
            match self.visibility {
                TextEntryVisibility::Visible => {
                    let content = {
                        if payload.placeholder.is_some() && payload.content.len().is_zero() {
                            let placeholder_content = payload.placeholder.unwrap();
                            placeholder_content.to_string()
                        } else {
                            payload.content.to_string()
                        }
                    };

                    log::trace!("action payload: {}", content);
                    if payload_chars < MAX_CHARS {
                        write!(tv.text, "{}", content).unwrap();
                    } else {
                        write!(tv.text, "...{}", &content[content.chars().count()-(MAX_CHARS - 3)..]).unwrap();
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                },
                TextEntryVisibility::Hidden => {
                    if payload_chars < MAX_CHARS {
                        for _char in payload.content.as_str().unwrap().chars() {
                            tv.text.push('*').expect("text field too long");
                        }
                    } else {
                        // just render a pure dummy string
                        tv.text.push('.').unwrap();
                        tv.text.push('.').unwrap();
                        tv.text.push('.').unwrap();
                        for _ in 0..(MAX_CHARS - 3) {
                            tv.text.push('*').expect("text field too long");
                        }
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                },
                TextEntryVisibility::LastChars => {
                    if payload_chars < MAX_CHARS {
                        let hide_to = if payload.content.as_str().unwrap().chars().count() >= 2 {
                            payload.content.as_str().unwrap().chars().count() - 2
                        } else {
                            0
                        };
                        for (index, ch) in payload.content.as_str().unwrap().chars().enumerate() {
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
                        let hide_to = if payload.content.as_str().unwrap().chars().count() >= 2 {
                            payload.content.as_str().unwrap().chars().count() - 2
                        } else {
                            0
                        };
                        for (index, ch) in payload.content.as_str().unwrap()[payload_chars-(MAX_CHARS - 3)..].chars().enumerate() {
                            if index + payload_chars-(MAX_CHARS - 3) < hide_to {
                                tv.text.push('*').expect("text field too long");
                            } else {
                                tv.text.push(ch).expect("text field too long");
                            }
                        }
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }
            }
            if self.is_password {
                let select_index = match self.visibility {
                    TextEntryVisibility::Visible => 0,
                    TextEntryVisibility::LastChars => 1,
                    TextEntryVisibility::Hidden => 2,
                };
                let prompt_width = glyph_to_height_hint(GlyphStyle::Monospace) as i16 * 4;
                let lr_margin = (modal.canvas_width - prompt_width * 3) / 2;
                let left_edge = lr_margin;

                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::GrowableFromTl(
                        Point::new(modal.margin, at_height + glyph_to_height_hint(GlyphStyle::Monospace) as i16 + modal.margin),
                        lr_margin as u16
                    ));
                tv.style = GlyphStyle::Large;
                tv.margin = Point::new(0, 0);
                tv.invert = self.is_password;
                tv.draw_border = false;
                tv.text.clear();
                write!(tv.text, "\u{2b05}").unwrap();
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");

                for i in 0..3 {
                    let mut tv = TextView::new(
                        modal.canvas,
                        TextBounds::GrowableFromTl(
                            Point::new(left_edge + i * prompt_width, at_height + glyph_to_height_hint(GlyphStyle::Monospace) as i16 + modal.margin),
                            prompt_width as u16)
                        );
                    tv.style = GlyphStyle::Monospace;
                    tv.margin = Point::new(8, 8);
                    if i == select_index {
                        tv.invert = !self.is_password;
                        tv.draw_border = true;
                        tv.rounded_border = Some(6);
                    } else {
                        tv.invert = self.is_password;
                        tv.draw_border = false;
                        tv.rounded_border = None;
                    }
                    tv.text.clear();
                    match i {
                        0 => write!(tv.text, "abcd").unwrap(),
                        1 => write!(tv.text, "ab**").unwrap(),
                        _ => write!(tv.text, "****").unwrap(),
                    }
                    modal.gam.post_textview(&mut tv).expect("couldn't post textview");
                }

                let mut tv = TextView::new(
                    modal.canvas,
                    TextBounds::GrowableFromTr(
                        Point::new(modal.canvas_width - modal.margin, at_height + glyph_to_height_hint(GlyphStyle::Monospace) as i16 + modal.margin),
                        lr_margin as u16
                    ));
                tv.style = GlyphStyle::Large;
                tv.margin = Point::new(0, 0);
                tv.invert = self.is_password;
                tv.draw_border = false;
                tv.text.clear();
                // minor bug - needs a trailing space on the right to make this emoji render. it's an issue in the word wrapper, but it's too late at night for me to figure this out right now.
                write!(tv.text, "\u{27a1} ").unwrap();
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            }

            // draw a line for where text gets entered (don't use a box, fitting could be awkward)
            modal.gam.draw_line(modal.canvas, Line::new_with_style(
                Point::new(left_text_margin, current_height + modal.line_height + 3),
                Point::new(modal.canvas_width - (modal.margin + bullet_margin), current_height + modal.line_height + 3),
                DrawStyle::new(color, color, 1))
                ).expect("couldn't draw entry line");

            current_height += self.field_height.get();
        }
    }
    fn key_action(&mut self, k: char) -> (Option<ValidatorErr>, bool) {
        // needs to be a reference, otherwise we're operating on a copy of the payload!
        let payload = &mut self.action_payloads[self.selected_field as usize];

        let can_move_downwards = !(self.selected_field+1 == self.max_field_amount as i16);
        let can_move_upwards =  !(self.selected_field-1 < 0);

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
                if let Some(validator) = self.validator {
                    if let Some(err_msg) = validator(*payload, self.action_opcode) {
                        payload.content.clear(); // reset the input field
                        return (Some(err_msg), false);
                    }
                }

                let mut payloads: TextEntryPayloads = Default::default();
                payloads.0[..self.max_field_amount as usize].copy_from_slice(&self.action_payloads[..self.max_field_amount as usize]);
                let buf = Buffer::into_buf(payloads).expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");

                for payload in self.action_payloads.iter_mut() {
                    payload.volatile_clear();
                }

                return (None, true)
            }
            '↑' => {
                if can_move_upwards {
                    self.selected_field -= 1
                }
            }
            '↓' => {
                if can_move_downwards {
                    self.selected_field += 1
                }
            }
            '\u{0}' => {
                // ignore null messages
            }
            '\u{8}' => { // backspace
                #[cfg(feature="tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(locales::t!("input.delete-tts", xous::LANG)).unwrap();
                }
                // coded in a conservative manner to avoid temporary allocations that can leave the plaintext on the stack
                if payload.content.len() > 0 { // don't backspace if we have no string.
                    let mut temp_str = String::<256>::from_str(payload.content.as_str().unwrap());
                    let cur_len = temp_str.as_str().unwrap().chars().count();
                    let mut c_iter = temp_str.as_str().unwrap().chars();
                    payload.content.clear();
                    for _ in 0..cur_len-1 {
                        payload.content.push(c_iter.next().unwrap()).unwrap();
                    }
                    temp_str.volatile_clear();
                }
            }
            _ => { // text entry
                #[cfg(feature="tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(&k.to_string()).unwrap();
                }
                    match k {
                        '\u{f701}' |  '\u{f700}' => (),
                    _ => {
                        payload.content.push(k).expect("ran out of space storing password");
                        log::trace!("****update payload: {}", payload.content);
                        payload.dirty = true;
                    }
                }

            }
        }
        (None, false)
    }
}