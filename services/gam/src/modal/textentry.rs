use graphics_server::api::GlyphStyle;
use crate::*;
use graphics_server::api::*;

use xous_ipc::{String, Buffer};
use num_traits::*;

use core::fmt::Write;

pub type ValidatorErr = xous_ipc::String::<256>;

#[derive(Debug, Copy, Clone, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum TextEntryVisibility {
    /// text is fully visible
    Visible = 0,
    /// only last chars are shown of text entry, the rest obscured with *
    LastChars = 1,
    /// all chars hidden as *
    Hidden = 2,
}
#[derive(Copy, Clone)]
pub struct TextEntry {
    pub is_password: bool,
    pub visibility: TextEntryVisibility,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: TextEntryPayload,
    // validator borrows the text entry payload, and returns an error message if something didn't go well.
    // validator takes as ragument the current action_payload, and the current action_opcode
    pub validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr> >,
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
        if self.is_password {
            glyph_height + 2*margin + glyph_height + 2*margin + 8 // 8 pixels extra margin because the emoji glyphs are oversized
        } else {
            glyph_height + 2*margin
        }
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
        tv.ellipsis = true;
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
                    write!(tv.text, "{}", self.action_payload.0.as_str().unwrap()).unwrap();
                } else {
                    write!(tv.text, "...{}", &self.action_payload.0.as_str().unwrap()[payload_chars-18..]).unwrap();
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
        if self.is_password {
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
        }

        // draw a line for where text gets entered (don't use a box, fitting could be awkward)
        modal.gam.draw_line(modal.canvas, Line::new_with_style(
            Point::new(modal.margin, at_height + modal.line_height + 4),
            Point::new(modal.canvas_width - modal.margin, at_height + modal.line_height + 4),
            DrawStyle::new(color, color, 1))
            ).expect("couldn't draw entry line");
    }
    fn key_action(&mut self, k: char) -> (Option<ValidatorErr>, bool) {
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
                    if let Some(err_msg) = validator(self.action_payload, self.action_opcode) {
                        self.action_payload.0.clear(); // reset the input field
                        return (Some(err_msg), false);
                    }
                }

                let buf = Buffer::into_buf(self.action_payload).expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");
                self.action_payload.volatile_clear(); // ensure the local copy of text is zero'd out
                return (None, true)
            }
            '↑' | '↓' => {
                // ignore these navigation keys
            }
            '\u{0}' => {
                // ignore null messages
            }
            '\u{8}' => { // backspace
                // coded in a conservative manner to avoid temporary allocations that can leave the plaintext on the stack
                if self.action_payload.0.len() > 0 { // don't backspace if we have no string.
                    let mut temp_str = String::<256>::from_str(self.action_payload.0.as_str().unwrap());
                    let cur_len = temp_str.as_str().unwrap().chars().count();
                    let mut c_iter = temp_str.as_str().unwrap().chars();
                    self.action_payload.0.clear();
                    for _ in 0..cur_len-1 {
                        self.action_payload.0.push(c_iter.next().unwrap()).unwrap();
                    }
                    temp_str.volatile_clear();
                }
            }
            _ => { // text entry
                self.action_payload.0.push(k).expect("ran out of space storing password");
                log::trace!("****update payload: {}", self.action_payload.0);
            }
        }
        (None, false)
    }
}