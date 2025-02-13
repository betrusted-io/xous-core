use crate::widgets::*;

#[derive(Debug)]
pub struct RadioButtons {
    pub items: Vec<ItemName>,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: RadioButtonPayload, // the current "radio button" selection
    pub select_index: i16,                  // the current candidate to be selected
    pub is_password: bool,
}
impl RadioButtons {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        RadioButtons {
            items: Vec::new(),
            action_conn,
            action_opcode,
            action_payload: RadioButtonPayload::new(""),
            select_index: 0,
            is_password: false,
        }
    }

    pub fn add_item(&mut self, new_item: ItemName) {
        if self.action_payload.as_str().len() == 0 {
            // default to the first item added
            self.action_payload = RadioButtonPayload::new(new_item.as_str());
        }
        self.items.push(new_item);
    }

    pub fn clear_items(&mut self) {
        self.items.clear();
        self.action_payload.clear();
    }
}

use crate::widgets::ActionApi;
impl ActionApi for RadioButtons {}
