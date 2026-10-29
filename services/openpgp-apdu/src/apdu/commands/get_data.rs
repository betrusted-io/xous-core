use crate::apdu::{CommandApdu, ResponseApdu, StatusWord, chunk_response};
use crate::openpgp::card::CardState;
use crate::openpgp::dos::get_do_data;

pub fn handle_get_data(cmd: &CommandApdu, card: &mut CardState) -> ResponseApdu {
    if !card.applet_selected {
        return ResponseApdu::error(StatusWord::SecurityStatusNotSatisfied);
    }
    let tag = u16::from(cmd.p1) << 8 | u16::from(cmd.p2);
    match get_do_data(tag, card.fixture) {
        Some(data) => chunk_response(cmd, card, data),
        // OpenPGP card: missing DO is 6A88 (not 6A83). GnuPG LEARN treats 6A83 as fatal.
        None => ResponseApdu::error(StatusWord::ReferenceDataNotFound),
    }
}
