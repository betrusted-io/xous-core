// SPDX-License-Identifier: Apache-2.0
//
//! Host-testable CCID bulk framing helpers (PC_to_RDR / RDR_to_PC wire format).

use std::collections::VecDeque;

/// Maximum assembled CCID message size on the wire (header + payload).
///
/// Option A (short APDU): keep equal to descriptor `dwMaxCCIDMessageLength`
/// (`0x10F` = 271). Short APDU max is 5+255 data + 10-byte CCID header = 270,
/// which fits. Extended APDU would need Option B (raise both + `dwFeatures`).
pub const CCID_WIRE_MAX: usize = 0x10F;
pub const CCID_HEADER_LEN: usize = 10;
/// Bulk endpoint wMaxPacketSize.
///
/// High-speed bulk MPS is 512 bytes (USB 2.0). CCID message max is
/// `CCID_WIRE_MAX` (271 bytes); frames larger than one packet are chunked
/// across multiple 512-byte bulk transfers. Dabao enumerates at high-speed.
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

/// PC_to_RDR_GetSlotStatus (0x65).
pub const PC_TO_RDR_GET_SLOT_STATUS: u8 = 0x65;
/// PC_to_RDR_IccPowerOn (0x62).
pub const PC_TO_RDR_ICC_POWER_ON: u8 = 0x62;
/// RDR_to_PC_SlotStatus (0x81).
pub const RDR_TO_PC_SLOT_STATUS: u8 = 0x81;
/// RDR_to_PC_DataBlock (0x80).
pub const RDR_TO_PC_DATA_BLOCK: u8 = 0x80;

/// OpenPGP smart-card ATR (T=1) returned inline for IccPowerOn.
/// Final byte `0x0C` is the TCK (exclusive-or of bytes from T0 through historical bytes).
pub const OPENPGP_ATR_LEN: usize = 21;
pub const OPENPGP_ATR: [u8; OPENPGP_ATR_LEN] = [
    0x3B, 0xDA, 0x18, 0xFF, 0x81, 0xB1, 0xFE, 0x75, 0x1F, 0x03, 0x00, 0x31, 0xC5, 0x73, 0xC0, 0x01, 0x40,
    0x00, 0x90, 0x00, 0x0C,
];

/// True if `frame` is a well-formed GetSlotStatus (header only, dwLength 0).
pub fn is_get_slot_status(frame: &[u8]) -> bool {
    if frame.len() != CCID_HEADER_LEN || frame[0] != PC_TO_RDR_GET_SLOT_STATUS {
        return false;
    }
    let dw = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
    dw == 0
}

/// True if `frame` is a well-formed IccPowerOn (header only, dwLength 0).
pub fn is_icc_power_on(frame: &[u8]) -> bool {
    if frame.len() != CCID_HEADER_LEN || frame[0] != PC_TO_RDR_ICC_POWER_ON {
        return false;
    }
    let dw = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
    dw == 0
}

/// Fixed RDR_to_PC_SlotStatus: command OK, ICC present/active (bStatus/bError/bClock = 0).
///
/// Built without heapless/usb-personality so the IRQ path can answer libccid's
/// CreateChannel GetSlotStatus probes (100 ms) without waking the stub.
pub fn rdr_to_pc_slot_status_ok(slot: u8, seq: u8) -> [u8; CCID_HEADER_LEN] {
    let mut frame = [0u8; CCID_HEADER_LEN];
    frame[0] = RDR_TO_PC_SLOT_STATUS;
    // dwLength = 0 already zeroed
    frame[5] = slot;
    frame[6] = seq;
    // bStatus = 0, bError = 0, bClockStatus = 0
    frame
}

/// RDR_to_PC_DataBlock carrying [`OPENPGP_ATR`] (IccPowerOn response).
///
/// Answered in IRQ context so pcscd/libccid get an ATR before the OpenPGP
/// process finishes vault init.
pub fn rdr_to_pc_data_block_atr(slot: u8, seq: u8) -> [u8; CCID_HEADER_LEN + OPENPGP_ATR_LEN] {
    let mut frame = [0u8; CCID_HEADER_LEN + OPENPGP_ATR_LEN];
    frame[0] = RDR_TO_PC_DATA_BLOCK;
    let dw = (OPENPGP_ATR_LEN as u32).to_le_bytes();
    frame[1] = dw[0];
    frame[2] = dw[1];
    frame[3] = dw[2];
    frame[4] = dw[3];
    frame[5] = slot;
    frame[6] = seq;
    // bStatus = 0, bError = 0, bChainParameter = 0
    frame[CCID_HEADER_LEN..].copy_from_slice(&OPENPGP_ATR);
    frame
}

pub fn make_icc_power_on(seq: u8) -> [u8; CCID_HEADER_LEN] {
    let mut frame = [0u8; CCID_HEADER_LEN];
    frame[0] = PC_TO_RDR_ICC_POWER_ON;
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

    #[test]
    fn get_slot_status_detect_and_reply() {
        let req = make_get_slot_status(7);
        assert!(is_get_slot_status(&req));
        let resp = rdr_to_pc_slot_status_ok(0, 7);
        assert_eq!(resp[0], RDR_TO_PC_SLOT_STATUS);
        assert_eq!(resp[5], 0);
        assert_eq!(resp[6], 7);
        assert_eq!(&resp[1..5], &[0, 0, 0, 0]);
        assert!(!is_get_slot_status(&resp));
    }

    #[test]
    fn empty_queue_pop_drops_refmut_before_reborrow() {
        // Mirrors CcidRxDeferred: edition 2021 keeps `if let Some(x) = cell.borrow_mut()...`
        // temporaries alive through the else branch, so a second borrow_mut panics.
        // Pop into a local first so the RefMut is dropped before the empty-queue path.
        use std::cell::RefCell;
        let q = RefCell::new(VecDeque::<Vec<u8>>::new());
        let queued = q.borrow_mut().pop_front();
        assert!(queued.is_none());
        q.borrow_mut().push_back(vec![0x6F]);
        assert_eq!(q.borrow_mut().pop_front().unwrap(), vec![0x6F]);
    }

    #[test]
    fn icc_power_on_detect_and_reply() {
        let req = make_icc_power_on(9);
        assert!(is_icc_power_on(&req));
        assert!(!is_get_slot_status(&req));
        let resp = rdr_to_pc_data_block_atr(0, 9);
        assert_eq!(resp[0], RDR_TO_PC_DATA_BLOCK);
        assert_eq!(resp[5], 0);
        assert_eq!(resp[6], 9);
        assert_eq!(u32::from_le_bytes([resp[1], resp[2], resp[3], resp[4]]) as usize, OPENPGP_ATR_LEN);
        assert_eq!(&resp[CCID_HEADER_LEN..], &OPENPGP_ATR);
        assert!(!is_icc_power_on(&resp));
    }
}
