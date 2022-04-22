use crate::*;

/// This is a specialty structure that takes input from the serial console and records it to a string.
#[derive(Debug, Copy, Clone)]
pub struct ConsoleInput {
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: TextEntryPayload,
}
impl ConsoleInput {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        ConsoleInput {
            action_conn,
            action_opcode,
            action_payload: TextEntryPayload::new(),
        }
    }
}
impl ActionApi for ConsoleInput {
    fn set_action_opcode(&mut self, op: u32) {self.action_opcode = op}
    fn height(&self, _glyph_height: i16, margin: i16) -> i16 {
        margin
    }
    fn redraw(&self, _at_height: i16, _modal: &Modal) {
        // has nothing
    }
    fn key_action(&mut self, k: char) -> (Option<ValidatorErr>, bool) {
        log::trace!("key_action: {}", k);
        match k {
            '\u{0}' => {
                // ignore null messages
            }
            '∴' | '\u{d}' => {
                let buf = Buffer::into_buf(self.action_payload).expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode).map(|_| ()).expect("couldn't send action message");
                return (None, true)
            }
            _ => { // text entry
                self.action_payload.content.push(k).expect("ran out of space storing password");
                log::trace!("****update payload: {}", self.action_payload.content);
            }
        }
        (None, false)
    }
}