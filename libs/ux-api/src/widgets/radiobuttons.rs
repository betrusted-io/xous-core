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
        RadioButtons {
            items: ScrollableList::default(),
            action_conn,
            action_opcode,
            action_payload: RadioButtonPayload::new(""),
        }
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
    fn height(&self, glyph_height: isize, margin: isize, _modal: &Modal) -> isize {
        // total items, then +1 for the "Okay" message
        (self.items.len() as isize + 1) * glyph_height + margin * 2 + margin * 2 + 5 // +4 for some bottom margin slop
    }

    fn redraw(&self, _at_height: isize, _modal: &Modal) { self.items.draw(); }

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
