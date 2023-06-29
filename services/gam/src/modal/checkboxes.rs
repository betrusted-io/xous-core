use crate::*;

use graphics_server::api::*;

use xous_ipc::Buffer;

use core::fmt::Write;
use locales::t;
#[cfg(feature="tts")]
use tts_frontend::TtsFrontend;

#[derive(Debug)]
pub struct CheckBoxes {
    pub items: Vec::<ItemName>,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: CheckBoxPayload,
    pub select_index: i16,
    gam: crate::Gam,
    #[cfg(feature = "tts")]
    pub tts: TtsFrontend,
}
impl CheckBoxes {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        #[cfg(feature="tts")]
        let tts = TtsFrontend::new(&xous_names::XousNames::new().unwrap()).unwrap();
        CheckBoxes {
            items: Vec::new(),
            action_conn,
            action_opcode,
            action_payload: CheckBoxPayload::new(),
            select_index: 0,
            gam: crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap(),
            #[cfg(feature="tts")]
            tts,
        }
    }
    pub fn add_item(&mut self, new_item: ItemName) {
        self.items.push(new_item);
    }
    pub fn clear_items(&mut self) {
        self.items.clear();
    }
}
impl ActionApi for CheckBoxes {
    fn set_action_opcode(&mut self, op: u32) {self.action_opcode = op}
    fn height(&self, glyph_height: i16, margin: i16, _modal: &Modal) -> i16 {
        // total items, then +1 for the "Okay" message
        (self.items.len() as i16 + 1) * glyph_height + margin * 2 + 5 // some slop needed because of the prompt character
    }
    fn redraw(&self, at_height: i16, modal: &Modal) {
        // prime a textview with the correct general style parameters
        let mut tv = TextView::new(
            modal.canvas,
            TextBounds::BoundingBox(Rectangle::new_coords(0, 0, 1, 1))
        );
        tv.ellipsis = true;
        tv.style = modal.style;
        tv.invert = false;
        tv.draw_border= false;
        tv.margin = Point::new(0, 0,);
        tv.insertion = None;

        let cursor_x = modal.margin;
        let select_x = modal.margin + 20;
        let text_x = modal.margin + 20 + 20;

        let emoji_slop = 2; // tweaked for a non-emoji glyph

        let mut cur_line = 0;
        let mut do_okay = true;
        for item in self.items.iter() {
            let cur_y = at_height + cur_line * modal.line_height;
            if cur_line == self.select_index {
                #[cfg(feature="tts")]
                {
                    self.tts.tts_simple(item.as_str()).unwrap();
                }
                // draw the cursor
                tv.text.clear();
                tv.bounds_computed = None;
                tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
                    Point::new(cursor_x, cur_y - emoji_slop), Point::new(cursor_x + 36, cur_y - emoji_slop + 36)
                ));
                write!(tv, "\u{25B6}").unwrap(); // right arrow
                modal.gam.post_textview(&mut tv).expect("couldn't post tv");
                do_okay = false;
            }
            if self.action_payload.contains(item.as_str()) {
                // draw the check mark
                tv.text.clear();
                tv.bounds_computed = None;
                tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
                    Point::new(select_x, cur_y - emoji_slop), Point::new(select_x + 36, cur_y + modal.line_height)
                ));
                write!(tv, "\u{d7}").unwrap(); // multiplication sign
                modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            }
            // draw the text
            tv.text.clear();
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
                Point::new(text_x, cur_y), Point::new(modal.canvas_width - modal.margin, cur_y + modal.line_height)
            ));
            write!(tv, "{}", item.as_str()).unwrap();
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");

            cur_line += 1;
        }
        cur_line += 1;
        let cur_y = at_height + cur_line * modal.line_height;
        if do_okay {
            tv.text.clear();
            tv.bounds_computed = None;
            tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
                Point::new(cursor_x, cur_y - emoji_slop), Point::new(cursor_x + 36, cur_y - emoji_slop + 36)
            ));
            write!(tv, "\u{25B6}").unwrap(); // right arrow emoji. use unicode numbers, because text editors do funny shit with emojis
            modal.gam.post_textview(&mut tv).expect("couldn't post tv");
            #[cfg(feature="tts")]
            {
                self.tts.tts_blocking(t!("checkbox.select_and_close_tts", locales::LANG)).unwrap();
                for item in self.action_payload.payload().iter() {
                    if let Some(name) = item {
                        self.tts.tts_blocking(name.as_str()).unwrap();
                    }
                }
            }
        }
        // draw the "OK" line
        tv.text.clear();
        tv.bounds_computed = None;
        tv.bounds_hint = TextBounds::BoundingBox(Rectangle::new(
            Point::new(text_x, cur_y), Point::new(modal.canvas_width - modal.margin, cur_y + modal.line_height)
        ));
        write!(tv, "{}", t!("radio.select_and_close", locales::LANG)).unwrap();
        modal.gam.post_textview(&mut tv).expect("couldn't post tv");

        // divider lines
        modal.gam.draw_line(modal.canvas, Line::new_with_style(
            Point::new(modal.margin, at_height),
            Point::new(modal.canvas_width - modal.margin, at_height),
            DrawStyle::new(PixelColor::Dark, PixelColor::Dark, 1))
            ).expect("couldn't draw entry line");
    }
    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        match k {
            '←' | '→' => {
                // ignore these navigation keys
            },
            '↑' => {
                if self.select_index > 0 {
                    self.select_index -= 1;
                }
            }
            '↓' => {
                if self.select_index < self.items.len() as i16 + 1 { // +1 is the "OK" button
                    self.select_index += 1;
                }
            }
            '∴' | '\u{d}' => {
                if (self.select_index as usize) < self.items.len() {
                    let item_name = self.items[self.select_index as usize].as_str();
                    if self.action_payload.contains(item_name) {
                        self.action_payload.remove(item_name);
                        #[cfg(feature="tts")]
                        {
                            self.tts.tts_blocking(t!("checkbox.uncheck", locales::LANG)).unwrap();
                            self.tts.tts_blocking(item_name).unwrap();
                        }
                    } else {
                        if !self.action_payload.add(item_name) {
                            log::warn!("Limit of {} items that can be checked hit, consider increasing MAX_ITEMS in gam/src/modal.rs", MAX_ITEMS);
                            log::warn!("The attempted item '{}' was not selected.", item_name);
                        } else {
                            #[cfg(feature="tts")]
                            {
                                self.tts.tts_blocking(t!("checkbox.check", locales::LANG)).unwrap();
                                self.tts.tts_blocking(item_name).unwrap();
                            }
                        }
                    }
                } else {  // the OK button select
                    // relinquish focus before returning the result
                    self.gam.relinquish_focus().unwrap();
                    xous::yield_slice();

                    let buf = Buffer::into_buf(self.action_payload).expect("couldn't convert message to payload");
                    buf.send(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");
                    return None;
                }
            }
            '\u{0}' => {
                // ignore null messages
            }
            _ => {
                // ignore text entry
            }
        }
        None
    }
}
