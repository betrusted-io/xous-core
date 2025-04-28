use locales::t;

use crate::minigfx::*;
use crate::widgets::*;

const CHECKED: &'static str = "\u{274E}"; // negative squared cross mark
const UNCHECKED: &'static str = "\u{2B1C}"; // white large square

#[derive(Debug)]
pub struct CheckBoxes {
    pub items: ScrollableList,
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: CheckBoxPayload,
}
impl CheckBoxes {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        let mut sl = ScrollableList::default().set_margin(Point::new(2, 0));
        let br = sl.pane().br();
        let row_height = sl.row_height();
        sl = sl.pane_size(Rectangle::new(Point::new(0, row_height as isize + 2), br));
        sl.add_item(0, t!("radio.select_and_close", locales::LANG));
        CheckBoxes { items: sl, action_conn, action_opcode, action_payload: CheckBoxPayload::new() }
    }

    pub fn add_item(&mut self, new_item: ItemName) {
        let unchecked_item = format!("{}{}", UNCHECKED, new_item.as_str());
        let list_len = self.items.col_length(0).unwrap_or(1);
        self.items.insert_item(0, list_len - 1, &unchecked_item);
    }

    pub fn clear_items(&mut self) { self.items.clear(); }
}

fn replace_first_char(s: &str, replacement: char) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(_) => std::iter::once(replacement).chain(chars).collect(),
        None => String::new(), // Empty string case
    }
}

use crate::widgets::ActionApi;
impl ActionApi for CheckBoxes {
    fn height(&self, _glyph_height: isize, _margin: isize, _modal: &Modal) -> isize {
        ((self.items.len() + 1) * self.items.row_height()) as isize
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
                let mut selected_item = self.items.get_selected().to_owned();
                if selected_item == t!("radio.select_and_close", locales::LANG) {
                    self.items.gfx.release_modal().unwrap();
                    xous::yield_slice();

                    let buf = xous_ipc::Buffer::into_buf(self.action_payload.clone())
                        .expect("couldn't convert message to payload");
                    buf.send(self.action_conn, self.action_opcode)
                        .map(|_| ())
                        .expect("couldn't send action message");
                    return None;
                }

                if selected_item.starts_with(UNCHECKED) {
                    // replace it with a checked version
                    selected_item = replace_first_char(&selected_item, CHECKED.chars().next().unwrap());
                    self.items.update_selected(&selected_item);
                    // strip the check/uncheck char
                    if let Some(first_char) = selected_item.chars().next() {
                        let len = first_char.len_utf8();
                        selected_item.replace_range(0..len, "");
                    }
                    if !self.action_payload.add(&selected_item) {
                        log::warn!(
                            "Limit of {} items that can be checked hit, consider increasing MAX_ITEMS in gam/src/modal.rs",
                            MAX_ITEMS
                        );
                        log::warn!("The attempted item '{}' was not selected.", selected_item);
                    }
                } else {
                    // replace it with an unchecked version
                    selected_item = replace_first_char(&selected_item, UNCHECKED.chars().next().unwrap());
                    self.items.update_selected(&selected_item);
                    // strip the check/uncheck char
                    if let Some(first_char) = selected_item.chars().next() {
                        let len = first_char.len_utf8();
                        selected_item.replace_range(0..len, "");
                    }
                    if self.action_payload.contains(&selected_item) {
                        self.action_payload.remove(&selected_item);
                    }
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
