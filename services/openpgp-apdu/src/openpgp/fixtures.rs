//! Hardcoded test-card profile for `gpg --card-status` bring-up.

use super::aid::TEST_AID_V1;

/// OpenPGP smart-card ATR (T=1) returned inline by `usb-bao1x` on IccPowerOn.
///
/// Must stay byte-identical to `services/usb-bao1x/src/ccid_framing.rs` `OPENPGP_ATR`.
pub const OPENPGP_ATR: [u8; 21] = [
    0x3B, 0xDA, 0x18, 0xFF, 0x81, 0xB1, 0xFE, 0x75, 0x1F, 0x03, 0x00, 0x31, 0xC5, 0x73, 0xC0, 0x01, 0x40,
    0x00, 0x90, 0x00, 0x0C,
];

const _: () = assert!(OPENPGP_ATR.len() == 21);

/// PW status bytes (DO C4): force-sig, max PW1, max RC, max PW3, retries x3.
pub const PW_STATUS_V1: [u8; 7] = [0x01, 254, 254, 254, 3, 3, 3];

/// Extended capabilities (DO C0) — conservative v3.4 defaults (no unimplemented features).
pub const EXTENDED_CAPABILITIES_V1: [u8; 10] = [0xB0, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

pub struct CardFixture {
    pub aid: [u8; 16],
    pub historical_bytes: &'static [u8],
    pub pw_status: &'static [u8; 7],
    pub extended_capabilities: &'static [u8; 10],
    pub name: &'static [u8],
    pub language: &'static [u8],
    pub sex: u8,
    pub url: &'static [u8],
    pub login_data: &'static [u8],
    pub signature_counter: u32,
}

pub static FIXTURE_V1_TEST: CardFixture = CardFixture {
    aid: TEST_AID_V1,
    historical_bytes: &OPENPGP_ATR,
    pw_status: &PW_STATUS_V1,
    extended_capabilities: &EXTENDED_CAPABILITIES_V1,
    name: b"",
    language: b"en",
    sex: 0x00,
    url: b"",
    login_data: b"",
    signature_counter: 0,
};
