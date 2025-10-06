use crate::signatures::Pubkey;

pub mod bao1;
pub mod bao2;
pub mod beta;
pub mod developer;

pub const BAO1_KEY_SLOT: usize = 0;
pub const BAO2_KEY_SLOT: usize = 1;
pub const BETA_KEY_SLOT: usize = 2;
pub const DEVELOPER_KEY_SLOT: usize = 3;

/// Nominal purposes of pub key slots:
///   - slot 0 is Baochip secured signing key 1
///   - slot 1 is Baochip secured signing key 2
///   - slot 2 is a Beta signing key. It's kept in a secured token but also available in a development
///     environment.
///   - slot 3 is the Developer signing key. It's a well-known key that anyone can use to sign an image. If
///     revoked, it is not a valid signing key. When not revoked, upon being presented with an image signed by
///     this, the bootloader will automatically erase all device-local secrets.
///
/// The dual signing keys for Baochip allows for a laddered upgrade path in case a signing key
/// needs to be replaced, upgraded, or cycled out for any reason.
pub const KEYSLOT_INITIAL_TAGS: [&'static [u8; 4]; 4] = [b"bao1", b"bao2", b"beta", b"dev "];

// helper function to pad arrays out, if needed. Previously used for auth_data until
// I realized that auth_data changes with every signing.
#[allow(dead_code)]
const fn pad_array<const N: usize, const M: usize>(input: &[u8; N]) -> [u8; M] {
    let mut result = [0u8; M];
    let mut i = 0;
    while i < N && i < M {
        result[i] = input[i];
        i += 1;
    }
    result
}

/// This is the exported record that should be copied into the header of all boot images
pub const PUBKEY_HEADER: [Pubkey; 4] = [bao1::PUBKEY, bao2::PUBKEY, beta::PUBKEY, developer::PUBKEY];
