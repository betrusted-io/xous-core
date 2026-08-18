//! OpenPGP application identifier (AID).

/// OpenPGP application AID prefix (RFC/OpenPGP card spec).
pub const OPENPGP_AID_PREFIX: &[u8] = &[0xD2, 0x76, 0x00, 0x01, 0x24];

/// GnuPG `openpgp_aid` SELECT value (6 bytes).
pub const OPENPGP_AID_SELECT: &[u8] = &[0xD2, 0x76, 0x00, 0x01, 0x24, 0x01];

pub const OPENPGP_CARD_VERSION_MAJOR: u8 = 0x03;
pub const OPENPGP_CARD_VERSION_MINOR: u8 = 0x04;

/// Full 16-byte OpenPGP AID for the v1 test card (3.4, manufacturer 0x0000, serial 0).
pub const TEST_AID_V1: [u8; 16] = [
    0xD2, 0x76, 0x00, 0x01, 0x24, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Build the 16-byte OpenPGP AID: prefix + version + manufacturer + serial + RFU.
pub fn build_aid(manufacturer_id: u16, serial: [u8; 4]) -> [u8; 16] {
    let mut aid = [0u8; 16];
    aid[0..5].copy_from_slice(OPENPGP_AID_PREFIX);
    aid[5] = OPENPGP_CARD_VERSION_MAJOR;
    aid[6] = OPENPGP_CARD_VERSION_MINOR;
    aid[7..9].copy_from_slice(&manufacturer_id.to_be_bytes());
    aid[9..13].copy_from_slice(&serial);
    aid
}

/// True when `received_aid` selects the OpenPGP application.
///
/// Accepts the 6-byte GnuPG SELECT AID, any prefix of the full 16-byte AID, or a
/// full AID with matching prefix and version bytes.
pub fn aid_matches_openpgp(received_aid: &[u8]) -> bool {
    if received_aid == OPENPGP_AID_SELECT {
        return true;
    }
    if received_aid.len() >= 7
        && received_aid.starts_with(OPENPGP_AID_PREFIX)
        && received_aid[5] == OPENPGP_CARD_VERSION_MAJOR
        && received_aid[6] == OPENPGP_CARD_VERSION_MINOR
    {
        return true;
    }
    received_aid.starts_with(OPENPGP_AID_PREFIX) && received_aid.len() >= OPENPGP_AID_PREFIX.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_gnupg_select_aid() {
        assert!(aid_matches_openpgp(OPENPGP_AID_SELECT));
    }
}
