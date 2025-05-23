use crate::minigfx::*;
use crate::widgets::*;

#[derive(Debug)]
pub struct RadioButtons {
    pub items: ScrollableList,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: RadioButtonPayload, // the current "radio button" selection
}
impl RadioButtons {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        let mut sl = ScrollableList::default().set_margin(Point::new(12, 0));
        let br = sl.pane().br();
        let row_height = sl.row_height();
        sl = sl.pane_size(Rectangle::new(Point::new(0, row_height as isize + 2), br));
        RadioButtons { items: sl, action_conn, action_opcode, action_payload: RadioButtonPayload::new("") }
    }

    pub fn add_item(&mut self, new_item: ItemName) {
        if self.action_payload.as_str().len() == 0 {
            // default to the first item added
            self.action_payload = RadioButtonPayload::new(new_item.as_str());
        }
        self.items.add_item(0, new_item.as_str());
    }

    pub fn clear_items(&mut self) {
        self.items.clear();
        self.action_payload.clear();
    }
}

use crate::widgets::ActionApi;
impl ActionApi for RadioButtons {
    fn height(&self, _glyph_height: isize, _margin: isize, _modal: &Modal) -> isize {
        (self.items.len() * self.items.row_height()) as isize
    }

    fn redraw(&self, at_height: isize, _modal: &Modal) { self.items.draw(at_height); }

    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        match k {
            '←' | '→' => {
                // ignore these navigation keys, we have only one column
            }
            '↑' | '↓' => {
                self.items.key_action(k);
            }
            '∴' | '\u{d}' => {
                self.action_payload = RadioButtonPayload::new(self.items.get_selected());

                self.items.gfx.release_modal().unwrap();
                xous::yield_slice();

                let buf = xous_ipc::Buffer::into_buf(self.action_payload.clone())
                    .expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode)
                    .map(|_| ())
                    .expect("couldn't send action message");
                return None;
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
