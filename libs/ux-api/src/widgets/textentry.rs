mod en;
use core::fmt::Write;

use locales::t;

use super::*;
use crate::minigfx::*;
use crate::service::api::Gid;

const MAX_FIELDS: i16 = 10;
pub const MAX_ITEMS: usize = 8;

pub type ValidatorErr = String;
pub type Payloads = [TextEntryPayload; MAX_FIELDS as usize];

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Eq, PartialEq, Default)]
pub struct TextEntryPayloads(Payloads, usize);

impl TextEntryPayloads {
    pub fn first(&self) -> TextEntryPayload { self.0[0].clone() }

    pub fn content(&self) -> Vec<TextEntryPayload> { self.0[..self.1].to_vec() }
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
    pub items: ScrollableList,
    // on-screen keyboard implementation
    pub osk: ScrollableList,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    // validator borrows the text entry payload, and returns an error message if something didn't go well.
    // validator takes as ragument the current action_payload, and the current action_opcode
    pub validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    pub action_payloads: Vec<TextEntryPayload>,

    osk_active: bool,
    osk_entry: String,
    osk_cursor: usize,
    max_field_amount: u32,
}

impl Default for TextEntry {
    fn default() -> Self {
        let mut sl = ScrollableList::default().set_margin(Point::new(4, 0));
        let br = sl.pane().br();
        let row_height = sl.row_height();
        sl = sl.pane_size(Rectangle::new(Point::new(0, row_height as isize + 2), br));

        let mut osk = ScrollableList::default().set_alignment(TextAlignment::Center);
        match locales::LANG {
            "en" => {
                let osk_matrix = en::OSK_MATRIX;
                for (col_num, col) in osk_matrix.iter().enumerate() {
                    for row in col {
                        osk.add_item(col_num, row);
                    }
                }
            }
            _ => unimplemented!("OSK matrix not yet implemented for language target"),
        }
        Self {
            items: sl,
            osk,
            osk_entry: String::new(),
            osk_cursor: 0,
            osk_active: false,
            action_conn: Default::default(),
            action_opcode: Default::default(),
            validator: Default::default(),
            action_payloads: Default::default(),
            max_field_amount: 0,
        }
    }
}

impl TextEntry {
    pub fn new(
        _visibility: TextEntryVisibility,
        action_conn: xous::CID,
        action_opcode: u32,
        action_payloads: Vec<TextEntryPayload>,
        validator: Option<fn(TextEntryPayload, u32) -> Option<ValidatorErr>>,
    ) -> Self {
        if action_payloads.len() as i16 > MAX_FIELDS {
            panic!("can't have more than {} fields, found {}", MAX_FIELDS, action_payloads.len());
        }

        let mut te = Self { action_conn, action_opcode, action_payloads, validator, ..Default::default() };

        for payload in te.action_payloads.iter() {
            te.items.add_item(0, payload.as_str());
        }
        te.items.add_item(0, t!("radio.select_and_close", locales::LANG));
        te
    }

    pub fn reset_action_payloads(&mut self, fields: u32, placeholders: Option<[Option<(String, bool)>; 10]>) {
        let mut payload = vec![TextEntryPayload::default(); fields as usize];

        if let Some(placeholders) = placeholders {
            for (index, element) in payload.iter_mut().enumerate() {
                if let Some((p, persist)) = &placeholders[index] {
                    element.placeholder = Some(p.to_string());
                    element.placeholder_persist = *persist;
                } else {
                    element.placeholder = None;
                    element.placeholder_persist = false;
                }
                element.insertion_point = None;
            }
        }

        self.action_payloads = payload;
        self.max_field_amount = fields;
        self.osk_entry.clear();
        self.osk_cursor = 0;

        self.items.clear();
        for payload in self.action_payloads.iter() {
            if let Some(placeholder) = &payload.placeholder {
                self.items.add_item(0, &placeholder);
            } else {
                self.items.add_item(0, "");
            }
        }
        self.items.add_item(0, t!("radio.select_and_close", locales::LANG));
    }
}

use crate::widgets::ActionApi;
impl ActionApi for TextEntry {
    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn height(&self, _glyph_height: isize, _margin: isize, _modal: &Modal) -> isize {
        ((1 + self.items.len()) * self.items.row_height()) as isize
    }

    fn redraw(&self, at_height: isize, _modal: &Modal) {
        if !self.osk_active {
            self.items.draw(at_height);
        } else {
            let pane = self.items.pane();
            // draw the active text in entry
            let mut tv = TextView::new(
                Gid::dummy(),
                TextBounds::BoundingBox(Rectangle::new(
                    pane.tl(),
                    pane.tl() + Point::new(pane.width() as isize, self.osk.row_height() as isize),
                )),
            );
            tv.margin = Point::new(0, 0);
            tv.style = self.osk.get_style();
            tv.ellipsis = true;
            tv.invert = false;
            tv.draw_border = false;
            tv.write_str(&self.osk_entry).ok();
            tv.insertion = Some(self.osk_cursor as i32);
            self.osk.gfx.draw_textview(&mut tv).ok();

            // now draw the OSK
            self.osk.draw(at_height + self.osk.row_height() as isize);
        }
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        if !self.osk_active {
            log::trace!("key_action: {}", k);
            match k {
                '←' | '→' | '↑' | '↓' => {
                    self.items.key_action(k);
                    None
                }
                '∴' | '\u{d}' => {
                    if self.items.get_selected_index().1 == self.items.len() - 1 {
                        // return and close - validate all fields and replace with defaults if 0-length
                        self.items.gfx.release_modal().unwrap();
                        xous::yield_slice();

                        let mut payloads: TextEntryPayloads = Default::default();
                        payloads.1 = self.max_field_amount as usize;
                        for (i, (dst, src)) in payloads.0[..self.max_field_amount as usize]
                            .iter_mut()
                            .zip(self.items.get_column(0).unwrap().iter())
                            .enumerate()
                        {
                            let mut payload = self.action_payloads[i].clone();
                            payload.content = src.to_owned();
                            *dst = payload;
                        }

                        let buf = xous_ipc::Buffer::into_buf(payloads)
                            .expect("couldn't convert message to payload");
                        buf.send(self.action_conn, self.action_opcode)
                            .map(|_| ())
                            .expect("couldn't send action message");

                        for payload in self.action_payloads.iter_mut() {
                            payload.volatile_clear();
                        }

                        None
                    } else {
                        // selected a field to go into osk mode
                        self.osk_entry = self.items.get_selected().to_owned();
                        self.osk_cursor = self.osk_entry.chars().count(); // park the cursor at the end
                        self.osk_active = true;
                        None
                    }
                }
                _ => None,
            }
        } else {
            // OSK handler
            match k {
                '←' | '→' | '↑' | '↓' => {
                    self.osk.key_action(k);
                }
                // select the character
                '∴' => {
                    let action = self.osk.get_selected();
                    if action == "⬅" {
                        if self.osk_cursor == 0 {
                            // replace the field with the defaults if we try to backspace in an empty field
                            self.osk_entry = self.items.get_selected().to_owned();
                            self.osk_cursor = self.osk_entry.chars().count(); // park the cursor at the end
                        } else {
                            if let Some((byte_start, ch)) =
                                self.osk_entry.char_indices().nth(self.osk_cursor.saturating_sub(1))
                            {
                                let byte_end = byte_start + ch.len_utf8();
                                self.osk_entry.replace_range(byte_start..byte_end, "");
                            }
                            self.osk_cursor = self.osk_cursor.saturating_sub(1);
                        }
                    } else {
                        if let Some(byte_pos) =
                            self.osk_entry.char_indices().nth(self.osk_cursor).map(|(i, _)| i).or_else(|| {
                                if self.osk_cursor == self.osk_entry.chars().count() {
                                    Some(self.osk_entry.len())
                                } else {
                                    None
                                }
                            })
                        {
                            self.osk_entry.insert_str(byte_pos, action);
                        } else {
                            self.osk_entry.push_str(action);
                        }
                        // increment the cursor position
                        self.osk_cursor = (self.osk_cursor + 1).min(self.osk_entry.chars().count());
                    }
                }
                // end the session by pressing the middle screen button
                '\u{d}' => {
                    self.osk_active = false;

                    // run the validator
                    let validation_copy = TextEntryPayload::new_with_fields(self.osk_entry.to_owned(), None);
                    if let Some(validator) = self.validator {
                        if let Some(err_msg) = validator(validation_copy, self.action_opcode) {
                            // return with error, and don't change the original fields
                            return Some(err_msg);
                        }
                    }
                    self.action_payloads[self.items.get_selected_index().1].dirty = true;
                    self.items.update_selected(&self.osk_entry);
                }
                _ => {
                    // ignore everything else
                }
            }
            None
        }
    }
}
