/// ===== PLACEHOLDER - bao2 signing HSM ceremony pending ======
#[allow(unused_imports)]
use super::*;
use crate::signatures::Pubkey;

/// Bao1 public key. Will be extracted from signing/bao1_id_ed25519_sk using extract_sk_credential.py
pub const ID_ED25519_SK_PUB: [u8; 32] = [0u8; 32];

/// Bao1 public key "key handle"
pub const ID_ED25519_SK_CRED_ID: [u8; 241] = [0u8; 241];
/// Bao1 public key "relying party"
pub const ID_ED25519_SK_RP: &'static str = "ssh:";

pub const PUBKEY: Pubkey = Pubkey {
    pk: ID_ED25519_SK_PUB,
    // Placeholder: tag is 0.
    tag: [0u8; 4], // *KEYSLOT_INITIAL_TAGS[BAO1_KEY_SLOT],
};
