use crate::apdu::{CommandApdu, ResponseApdu, handle_get_response};
use crate::openpgp::card::CardState;

pub fn handle_get_response_cmd(cmd: &CommandApdu, card: &mut CardState) -> ResponseApdu {
    handle_get_response(cmd, card)
}
