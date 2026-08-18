use crate::apdu::{chunk_response, CommandApdu, ResponseApdu, StatusWord};
use crate::openpgp::card::CardState;
use crate::openpgp::dos::get_do_data;

pub fn handle_get_data(cmd: &CommandApdu, card: &mut CardState) -> ResponseApdu {
    if !card.applet_selected {
        return ResponseApdu::error(StatusWord::SecurityStatusNotSatisfied);
    }
    let tag = u16::from(cmd.p1) << 8 | u16::from(cmd.p2);
    match get_do_data(tag, card.fixture) {
        Some(data) => chunk_response(cmd, card, data),
        None => ResponseApdu::error(StatusWord::RecordNotFound),
    }
}
