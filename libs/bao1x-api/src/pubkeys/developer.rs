use super::*;
use crate::signatures::Pubkey;

/// Developer public key. The private key is the devkey/ directory, included here for your
/// convenience: "MC4CAQAwBQYDK2VwBCIEIKindlyNoteThisIsADevKeyDontUseForProduction" (base64)
pub const PUB: [u8; 32] = [
    0x1c, 0x9b, 0xea, 0xe3, 0x2a, 0xea, 0xc8, 0x75, 0x07, 0xc1, 0x80, 0x94, 0x38, 0x7e, 0xff, 0x1c, 0x74,
    0x61, 0x42, 0x82, 0xaf, 0xfd, 0x81, 0x52, 0xd8, 0x71, 0x35, 0x2e, 0xdf, 0x3f, 0x58, 0xbb,
];
/// Developer key has no auth data. It is a classic ed25519ph signature.
pub const AUTH_DATA: [u8; 0] = [];

pub const PUBKEY: Pubkey = Pubkey {
    pk: PUB,
    tag: *KEYSLOT_INITIAL_TAGS[DEVELOPER_KEY_SLOT],
    aad_len: AUTH_DATA.len() as u8,
    aad: pad_array(&AUTH_DATA),
};
