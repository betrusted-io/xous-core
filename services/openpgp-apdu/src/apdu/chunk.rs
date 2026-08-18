//! Response chunking for `61 XX` / GET RESPONSE continuation.

use super::parse::{CommandApdu, ResponseApdu};
use super::status::StatusWord;
use crate::openpgp::card::CardState;

pub fn le_limit(cmd: &CommandApdu) -> usize {
    cmd.le
        .map(|l| {
            if l == 0 {
                256usize
            } else {
                usize::from(l.min(512))
            }
        })
        .unwrap_or(256)
}

pub fn chunk_response(cmd: &CommandApdu, card: &mut CardState, data: Vec<u8>) -> ResponseApdu {
    let lim = le_limit(cmd);
    if data.len() <= lim {
        return ResponseApdu::ok(data);
    }
    card.response_buffer = data;
    card.response_offset = 0;
    emit_chunk(card, lim)
}

pub fn handle_get_response(cmd: &CommandApdu, card: &mut CardState) -> ResponseApdu {
    let lim = le_limit(cmd);
    let remaining = card.response_buffer.len().saturating_sub(card.response_offset);
    if remaining == 0 {
        return ResponseApdu::error(StatusWord::WrongParametersP1P2);
    }
    emit_chunk(card, lim)
}

fn emit_chunk(card: &mut CardState, lim: usize) -> ResponseApdu {
    let remaining = card.response_buffer.len().saturating_sub(card.response_offset);
    let take = remaining.min(lim);
    let chunk = card.response_buffer[card.response_offset..card.response_offset + take].to_vec();
    card.response_offset += take;
    let left = card.response_buffer.len().saturating_sub(card.response_offset);
    if left > 0 {
        let n = u8::try_from(left.min(255)).unwrap_or(255);
        ResponseApdu::with_status(chunk, StatusWord::MoreDataAvailable(n))
    } else {
        card.response_buffer.clear();
        card.response_offset = 0;
        ResponseApdu::ok(chunk)
    }
}
