use crate::apdu::{CommandApdu, ResponseApdu, StatusWord};

pub fn handle_verify(cmd: &CommandApdu) -> ResponseApdu {
    match cmd.p2 {
        0x81 | 0x82 | 0x83 => ResponseApdu::ok_empty(),
        _ => ResponseApdu::error(StatusWord::WrongParametersP1P2),
    }
}
