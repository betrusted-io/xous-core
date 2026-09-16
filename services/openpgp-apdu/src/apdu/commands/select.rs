use crate::apdu::{CommandApdu, ResponseApdu, StatusWord};
use crate::openpgp::aid::aid_matches_openpgp;
use crate::openpgp::card::CardState;

pub fn handle_select(cmd: &CommandApdu, card: &mut CardState) -> ResponseApdu {
    if cmd.p1 != 0x04 || (cmd.p2 != 0x00 && cmd.p2 != 0x04) {
        return ResponseApdu::error(StatusWord::WrongParametersP1P2);
    }
    if !aid_matches_openpgp(&cmd.data) {
        return ResponseApdu::error(StatusWord::FileNotFound);
    }
    card.applet_selected = true;
    card.clear_chunk_state();
    ResponseApdu::ok_empty()
}
