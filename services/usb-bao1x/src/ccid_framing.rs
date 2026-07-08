// SPDX-License-Identifier: Apache-2.0
//
//! Host-testable CCID bulk framing helpers (PC_to_RDR / RDR_to_PC wire format).

use std::collections::VecDeque;

/// Maximum assembled CCID message size on the wire (header + payload).
pub const CCID_WIRE_MAX: usize = 530;
pub const CCID_HEADER_LEN: usize = 10;
pub const CCID_BULK_MAX_PACKET: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcidFramingError {
    Overflow,
}

/// Total frame length from the first 10 bytes of a CCID message, if parseable.
pub fn frame_total_len(rx_assembly: &[u8]) -> Option<usize> {
    if rx_assembly.len() < CCID_HEADER_LEN {
        return None;
    }
    let dw_len =
        u32::from_le_bytes([rx_assembly[1], rx_assembly[2], rx_assembly[3], rx_assembly[4]]) as usize;
    let total = CCID_HEADER_LEN.saturating_add(dw_len);
    if total > CCID_WIRE_MAX {
        return None;
    }
    Some(total)
}

/// Append a bulk-OUT chunk; returns `Err` if the assembly buffer would exceed `CCID_WIRE_MAX`.
pub fn append_bulk_out(rx_assembly: &mut Vec<u8>, chunk: &[u8]) -> Result<(), CcidFramingError> {
    if rx_assembly.len().saturating_add(chunk.len()) > CCID_WIRE_MAX {
        rx_assembly.clear();
        return Err(CcidFramingError::Overflow);
    }
    rx_assembly.extend_from_slice(chunk);
    Ok(())
}

/// Drain every complete frame currently present in `rx_assembly` into `complete_rx`.
pub fn drain_complete_frames(rx_assembly: &mut Vec<u8>, complete_rx: &mut VecDeque<Vec<u8>>) {
    loop {
        let Some(total) = frame_total_len(rx_assembly) else {
            if rx_assembly.len() >= CCID_HEADER_LEN {
                rx_assembly.clear();
            }
            break;
        };
        if rx_assembly.len() < total {
            break;
        }
        let frame = rx_assembly[..total].to_vec();
        rx_assembly.drain(..total);
        complete_rx.push_back(frame);
    }
}

/// Split `tx_buf` into the next bulk-IN chunk (at most `CCID_BULK_MAX_PACKET` bytes).
pub fn next_tx_chunk(tx_buf: &[u8]) -> Option<&[u8]> {
    if tx_buf.is_empty() {
        None
    } else {
        let n = tx_buf.len().min(CCID_BULK_MAX_PACKET);
        Some(&tx_buf[..n])
    }
}

/// Remove the first `n` bytes from `tx_buf` after a successful bulk-IN write.
pub fn consume_tx_chunk(tx_buf: &mut Vec<u8>, n: usize) {
    if n >= tx_buf.len() {
        tx_buf.clear();
    } else {
        tx_buf.drain(..n);
    }
}

/// Provisioning complete marker stored in PDDB (`usb.ccid` / `provisioned`).
pub const CCID_PROVISIONED_MARKER: &[u8] = b"OKV1";

pub fn is_provisioned_marker(buf: &[u8]) -> bool {
    buf.len() >= CCID_PROVISIONED_MARKER.len() && buf.starts_with(CCID_PROVISIONED_MARKER)
}

pub fn make_get_slot_status(seq: u8) -> [u8; CCID_HEADER_LEN] {
    let mut frame = [0u8; CCID_HEADER_LEN];
    frame[0] = 0x65;
    frame[5] = 0;
    frame[6] = seq;
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_header_yields_no_frame() {
        let mut asm = vec![0x65, 0, 0, 0, 0];
        let mut out = VecDeque::new();
        drain_complete_frames(&mut asm, &mut out);
        assert!(out.is_empty());
        assert_eq!(asm.len(), 5);
    }

    #[test]
    fn valid_get_slot_status_frame() {
        let mut asm = make_get_slot_status(3).to_vec();
        let mut out = VecDeque::new();
        drain_complete_frames(&mut asm, &mut out);
        assert!(asm.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0][0], 0x65);
        assert_eq!(out[0][6], 3);
    }

    #[test]
    fn oversize_dw_length_clears_buffer() {
        let mut asm = vec![0x6F, 0xFF, 0x02, 0, 0, 0, 0, 0, 0, 0];
        let mut out = VecDeque::new();
        drain_complete_frames(&mut asm, &mut out);
        assert!(asm.is_empty());
        assert!(out.is_empty());
    }

    #[test]
    fn split_bulk_out_reassembles() {
        let frame = make_get_slot_status(1);
        let mut asm = Vec::new();
        append_bulk_out(&mut asm, &frame[..4]).unwrap();
        append_bulk_out(&mut asm, &frame[4..]).unwrap();
        let mut out = VecDeque::new();
        drain_complete_frames(&mut asm, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_slice(), frame.as_slice());
    }

    #[test]
    fn append_overflow_clears() {
        let mut asm = vec![0u8; CCID_WIRE_MAX];
        assert_eq!(append_bulk_out(&mut asm, &[1]), Err(CcidFramingError::Overflow));
        assert!(asm.is_empty());
    }

    #[test]
    fn provisioned_marker() {
        assert!(is_provisioned_marker(b"OKV1"));
        assert!(is_provisioned_marker(b"OKV1extra"));
        assert!(!is_provisioned_marker(b"OK"));
        assert!(!is_provisioned_marker(b""));
    }

    #[test]
    fn tx_chunking() {
        let buf = vec![0xAB; 600];
        let chunk_len = next_tx_chunk(&buf).unwrap().len();
        assert_eq!(chunk_len, CCID_BULK_MAX_PACKET);
        let mut remain = buf;
        consume_tx_chunk(&mut remain, chunk_len);
        assert_eq!(remain.len(), 600 - CCID_BULK_MAX_PACKET);
    }
}
