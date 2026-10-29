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

/// Historical bytes for DO 5F52 — the compact-TLV historical portion of `OPENPGP_ATR`,
/// not the full ATR. Category indicator `0x00` (status included).
///
/// ATR layout: TS T0 TA1.. + historical `00 31 C5 73 C0 01 40 00 90 00` + TCK.
pub const HISTORICAL_BYTES_V1: [u8; 10] = [0x00, 0x31, 0xC5, 0x73, 0xC0, 0x01, 0x40, 0x00, 0x90, 0x00];

/// PW status bytes (DO C4): force-sig, max PW1, max RC, max PW3, retries x3.
pub const PW_STATUS_V1: [u8; 7] = [0x01, 254, 254, 254, 3, 3, 3];

/// Extended capabilities (DO C0) — conservative v3.4 defaults (no unimplemented features).
pub const EXTENDED_CAPABILITIES_V1: [u8; 10] = [0xB0, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Algorithm attributes (DO C1/C2/C3): RSA-2048, e=32-bit, standard import format.
/// OpenPGP card 3.4.1 §4.4.1: alg(01) | n_bits | e_bits | format(00).
pub const ALGO_ATTR_RSA2048: [u8; 6] = [0x01, 0x08, 0x00, 0x00, 0x20, 0x00];

pub struct CardFixture {
    pub aid: [u8; 16],
    pub historical_bytes: &'static [u8],
    pub pw_status: &'static [u8; 7],
    pub extended_capabilities: &'static [u8; 10],
    pub algo_attr_sig: &'static [u8],
    pub algo_attr_dec: &'static [u8],
    pub algo_attr_aut: &'static [u8],
    pub name: &'static [u8],
    pub language: &'static [u8],
    pub sex: u8,
    pub url: &'static [u8],
    pub login_data: &'static [u8],
    pub signature_counter: u32,
}

pub static FIXTURE_V1_TEST: CardFixture = CardFixture {
    aid: TEST_AID_V1,
    historical_bytes: &HISTORICAL_BYTES_V1,
    pw_status: &PW_STATUS_V1,
    extended_capabilities: &EXTENDED_CAPABILITIES_V1,
    algo_attr_sig: &ALGO_ATTR_RSA2048,
    algo_attr_dec: &ALGO_ATTR_RSA2048,
    algo_attr_aut: &ALGO_ATTR_RSA2048,
    name: b"",
    language: b"en",
    sex: 0x00,
    url: b"",
    login_data: b"",
    signature_counter: 0,
};
