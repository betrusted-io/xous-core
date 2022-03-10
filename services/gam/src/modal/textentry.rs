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
            glyph_height + 2*margin + glyph_height
        } else {
            glyph_height + 2*margin
        }
    }
    fn redraw(&self, at_height: i16, modal: &Modal) {
        const MAX_CHARS: usize = 33;
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
        tv.style = if self.is_password {
            GlyphStyle::Monospace
        } else {
            modal.style
        };
        tv.margin = Point::new(0, 0);
        tv.draw_border = false;
        tv.insertion = Some(self.action_payload.0.len() as i32);
        tv.text.clear(); // make sure this is blank
        let payload_chars = self.action_payload.0.as_str().unwrap().chars().count();
        // TODO: condense the "above MAX_CHARS" chars length path a bit -- written out "the dumb way" just to reason out the logic a bit
        match self.visibility {
            TextEntryVisibility::Visible => {
                log::trace!("action payload: {}", self.action_payload.0.as_str().unwrap());
                if payload_chars < MAX_CHARS {
                    write!(tv.text, "{}", self.action_payload.0.as_str().unwrap()).unwrap();
                } else {
                    write!(tv.text, "...{}", &self.action_payload.0.as_str().unwrap()[payload_chars-(MAX_CHARS - 3)..]).unwrap();
                }
                modal.gam.post_textview(&mut tv).expect("couldn't post textview");
            },
            TextEntryVisibility::Hidden => {
                if payload_chars < MAX_CHARS {
                    for _char in self.action_payload.0.as_str().unwrap().chars() {
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
                    for (index, ch) in self.action_payload.0.as_str().unwrap()[payload_chars-(MAX_CHARS - 3)..].chars().enumerate() {
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
                #[cfg(feature="tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(locales::t!("input.delete-tts", xous::LANG)).unwrap();
                }
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
                #[cfg(feature="tts")]
                {
                    let xns = xous_names::XousNames::new().unwrap();
                    let tts = tts_frontend::TtsFrontend::new(&xns).unwrap();
                    tts.tts_blocking(&k.to_string()).unwrap();
                }
                self.action_payload.0.push(k).expect("ran out of space storing password");
                log::trace!("****update payload: {}", self.action_payload.0);
            }
        }
        (None, false)
    }
}