//! Parse and build CCID bulk frames (USB CCID 1.1).

const HDR_LEN: usize = 10;

#[derive(Debug, Eq, PartialEq)]
pub enum PcToRdr {
    IccPowerOn { slot: u8, seq: u8, power_select: u8 },
    IccPowerOff { slot: u8, seq: u8 },
    GetSlotStatus { slot: u8, seq: u8 },
    XfrBlock { slot: u8, seq: u8, apdu: Vec<u8> },
    Abort { slot: u8, seq: u8 },
}

impl PcToRdr {
    pub fn answered_inline_by_usb_bao1x(&self) -> bool {
        matches!(self, Self::IccPowerOn { .. } | Self::GetSlotStatus { .. })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CcidError {
    TooShort,
    LengthMismatch,
    UnknownMessageType,
    PayloadTooLarge,
}

pub const RDR_TO_PC_DATA_BLOCK: u8 = 0x80;
pub const RDR_TO_PC_SLOT_STATUS: u8 = 0x81;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CcidStatus {
    pub b_status: u8,
    pub b_error: u8,
    pub b_chain: u8,
}

impl CcidStatus {
    pub const fn ok_active() -> Self { Self { b_status: 0x00, b_error: 0x00, b_chain: 0x00 } }

    pub const fn cmd_not_supported() -> Self { Self { b_status: 0x40, b_error: 0xFE, b_chain: 0x00 } }
}

pub fn parse_pc_to_rdr(data: &[u8]) -> Result<PcToRdr, CcidError> {
    if data.len() < HDR_LEN {
        return Err(CcidError::TooShort);
    }
    let msg_type = data[0];
    let dw_length = u32::from_le_bytes(data[1..5].try_into().map_err(|_| CcidError::TooShort)?);
    let slot = data[5];
    let seq = data[6];
    let b7 = data[7];

    let expected = HDR_LEN.saturating_add(dw_length as usize);
    if data.len() != expected {
        return Err(CcidError::LengthMismatch);
    }

    match msg_type {
        0x62 => {
            if dw_length != 0 {
                return Err(CcidError::LengthMismatch);
            }
            Ok(PcToRdr::IccPowerOn { slot, seq, power_select: b7 })
        }
        0x63 => {
            if dw_length != 0 {
                return Err(CcidError::LengthMismatch);
            }
            Ok(PcToRdr::IccPowerOff { slot, seq })
        }
        0x65 => {
            if dw_length != 0 {
                return Err(CcidError::LengthMismatch);
            }
            Ok(PcToRdr::GetSlotStatus { slot, seq })
        }
        0x6F => {
            let payload = &data[HDR_LEN..];
            if payload.len() > 512 {
                return Err(CcidError::PayloadTooLarge);
            }
            Ok(PcToRdr::XfrBlock { slot, seq, apdu: payload.to_vec() })
        }
        0x72 => {
            if dw_length != 0 {
                return Err(CcidError::LengthMismatch);
            }
            Ok(PcToRdr::Abort { slot, seq })
        }
        _ => Err(CcidError::UnknownMessageType),
    }
}

fn push_hdr(out: &mut Vec<u8>, msg_type: u8, dw_length: u32, slot: u8, seq: u8, st: &CcidStatus) {
    out.push(msg_type);
    out.extend_from_slice(&dw_length.to_le_bytes());
    out.push(slot);
    out.push(seq);
    out.push(st.b_status);
    out.push(st.b_error);
    out.push(st.b_chain);
}

pub fn rdr_to_pc_data_block(slot: u8, seq: u8, status: CcidStatus, apdu_response: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + apdu_response.len());
    push_hdr(&mut out, RDR_TO_PC_DATA_BLOCK, apdu_response.len() as u32, slot, seq, &status);
    out.extend_from_slice(apdu_response);
    out
}

pub fn rdr_to_pc_slot_status(slot: u8, seq: u8, status: CcidStatus) -> Vec<u8> {
    let mut out = Vec::with_capacity(HDR_LEN);
    push_hdr(&mut out, RDR_TO_PC_SLOT_STATUS, 0, slot, seq, &status);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xfr_block_select() {
        let apdu = [0x00u8, 0xA4, 0x04, 0x00, 0x06, 0xD2, 0x76, 0x00, 0x01, 0x24, 0x01];
        let mut frame = vec![0x6F, 0x0B, 0, 0, 0, 0, 2, 0, 0, 0];
        frame.extend_from_slice(&apdu);
        match parse_pc_to_rdr(&frame).unwrap() {
            PcToRdr::XfrBlock { apdu: a, .. } => assert_eq!(a, apdu),
            _ => panic!("expected XfrBlock"),
        }
    }
}
