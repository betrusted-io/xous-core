use crate::apdu::commands::{handle_get_data, handle_get_response_cmd, handle_select, handle_verify};
use crate::apdu::{CommandApdu, ResponseApdu, StatusWord};
use crate::openpgp::card::CardState;

const INS_SELECT: u8 = 0xA4;
const INS_VERIFY: u8 = 0x20;
const INS_GET_RESPONSE: u8 = 0xC0;
const INS_GET_DATA: u8 = 0xCA;

pub fn dispatch_apdu(cmd: CommandApdu, card: &mut CardState) -> ResponseApdu {
    match cmd.ins {
        INS_GET_RESPONSE => handle_get_response_cmd(&cmd, card),
        INS_SELECT => handle_select(&cmd, card),
        INS_VERIFY => handle_verify(&cmd),
        INS_GET_DATA => handle_get_data(&cmd, card),
        _ => ResponseApdu::error(StatusWord::InstructionNotSupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apdu::CommandApdu;
    use crate::openpgp::fixtures::FIXTURE_V1_TEST;

    #[test]
    fn select_then_get_aid() {
        let mut card = CardState::new(&FIXTURE_V1_TEST);
        let sel =
            CommandApdu::parse(&[0x00, 0xA4, 0x04, 0x00, 0x06, 0xD2, 0x76, 0x00, 0x01, 0x24, 0x01]).unwrap();
        let r = dispatch_apdu(sel, &mut card);
        assert_eq!(r.sw1, 0x90);

        let gd = CommandApdu::parse(&[0x00, 0xCA, 0x00, 0x4F, 0x00]).unwrap();
        let r2 = dispatch_apdu(gd, &mut card);
        assert_eq!(r2.sw1, 0x90);
        assert!(!r2.data.is_empty());
    }
}
