use crate::widgets::*;

#[derive(Debug)]
pub struct CheckBoxes {
    pub items: Vec<ItemName>,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: CheckBoxPayload,
    pub select_index: i16,
}
impl CheckBoxes {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        CheckBoxes {
            items: Vec::new(),
            action_conn,
            action_opcode,
            action_payload: CheckBoxPayload::new(),
            select_index: 0,
        }
    }

    pub fn add_item(&mut self, new_item: ItemName) { self.items.push(new_item); }

    pub fn clear_items(&mut self) { self.items.clear(); }
}

use crate::widgets::ActionApi;
impl ActionApi for CheckBoxes {}
