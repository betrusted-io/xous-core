use crate::*;

/// This is a specialty structure that takes input from the serial console and records it to a string.
#[derive(Debug)]
pub struct ConsoleInput {
    pub action_conn: xous::CID,
    pub action_opcode: u32,
    pub action_payload: TextEntryPayload,
    gam: Gam,
}
impl Clone for ConsoleInput {
    fn clone(&self) -> Self {
        ConsoleInput {
            action_conn: self.action_conn,
            action_opcode: self.action_opcode,
            action_payload: self.action_payload.clone(),
            gam: crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap(),
        }
    }
}
impl ConsoleInput {
    pub fn new(action_conn: xous::CID, action_opcode: u32) -> Self {
        ConsoleInput {
            action_conn,
            action_opcode,
            action_payload: TextEntryPayload::new(),
            gam: crate::Gam::new(&xous_names::XousNames::new().unwrap()).unwrap(),
        }
    }
}
impl ActionApi for ConsoleInput {
    fn set_action_opcode(&mut self, op: u32) { self.action_opcode = op }

    fn height(&self, _glyph_height: i16, margin: i16, _modal: &Modal) -> i16 { margin }

    fn redraw(&self, _at_height: i16, _modal: &Modal) {
        // has nothing
    }

    fn key_action(&mut self, k: char) -> Option<ValidatorErr> {
        log::trace!("key_action: {}", k);
        match k {
            '\u{0}' => {
                // ignore null messages
            }
            '∴' | '\u{d}' => {
                // relinquish focus before returning the result
                self.gam.relinquish_focus().unwrap();
                xous::yield_slice();

                let buf = Buffer::into_buf(self.action_payload.clone())
                    .expect("couldn't convert message to payload");
                buf.send(self.action_conn, self.action_opcode)
                    .map(|_| ())
                    .expect("couldn't send action message");
                return None;
            }
            _ => {
                // text entry
                self.action_payload.content.push(k);
                log::trace!("****update payload: {}", self.action_payload.content);
            }
        }
        None
    }
}
