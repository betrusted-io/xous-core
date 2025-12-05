use crate::signatures::{FunctionCode, Pubkey};

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

/// This structure defines a security configuration
///
/// `image_ptr`: a `*const u32` pointer to the base of the firmware image being configured
/// `pubkey_ptr`: a `*const u32` pointer to the public keys used to validate the image at `image_ptr`
/// `revocation_owc`: a `usize` number that is the one-way counter index that defines the beginning of the
/// primary revocation counter bank. The duplicate bank is inferred based on the expected fixed offset to
/// the duplicate bank.
/// `function_codes`: a `&'static [u32]` that contains the allowable function codes for the image. The
/// function codes are a domain separator that prevent an image meant for one stage of boot being used for
/// another.
#[derive(Copy, Clone)]
pub struct SecurityConfiguration {
    pub image_ptr: *const u32,
    pub pubkey_ptr: *const u32,
    pub revocation_owc: usize,
    pub function_codes: &'static [u32],
}

pub const BOOT0_SELF_CHECK: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::BOOT0_START as *const u32,
    pubkey_ptr: crate::BOOT0_START as *const u32,
    revocation_owc: crate::BOOT0_REVOCATION_OFFSET,
    function_codes: &[FunctionCode::Boot0 as u32],
};

pub const BOOT0_TO_BOOT1: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::BOOT1_START as *const u32,
    pubkey_ptr: crate::BOOT0_START as *const u32,
    revocation_owc: crate::BOOT0_REVOCATION_OFFSET,
    function_codes: &[
        FunctionCode::Boot1 as u32,
        FunctionCode::UpdatedBoot1 as u32,
        FunctionCode::Developer as u32,
    ],
};

/// This is different from a jump directly to loader/baremetal because the
/// function codes *must* be for Boot1.
pub const BOOT0_TO_ALTBOOT1: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::LOADER_START as *const u32,
    pubkey_ptr: crate::BOOT0_START as *const u32,
    revocation_owc: crate::BOOT0_REVOCATION_OFFSET,
    function_codes: &[
        FunctionCode::Boot1 as u32,
        FunctionCode::UpdatedBoot1 as u32,
        FunctionCode::Developer as u32,
    ],
};

pub const BOOT1_TO_LOADER_OR_BAREMETAL: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::LOADER_START as *const u32,
    pubkey_ptr: crate::BOOT1_START as *const u32,
    revocation_owc: crate::BOOT1_REVOCATION_OFFSET,
    function_codes: &[
        FunctionCode::Baremetal as u32,
        FunctionCode::UpdatedBaremetal as u32,
        FunctionCode::Loader as u32,
        FunctionCode::UpdatedLoader as u32,
        FunctionCode::Developer as u32,
    ],
};

pub const LOADER_TO_KERNEL: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::KERNEL_START as *const u32,
    pubkey_ptr: crate::LOADER_START as *const u32,
    revocation_owc: crate::LOADER_REVOCATION_OFFSET,
    function_codes: &[
        FunctionCode::Kernel as u32,
        FunctionCode::UpdatedKernel as u32,
        FunctionCode::Developer as u32,
    ],
};

pub const LOADER_TO_DETACHED_APP: SecurityConfiguration = SecurityConfiguration {
    image_ptr: crate::offsets::dabao::APP_RRAM_START as *const u32,
    pubkey_ptr: crate::LOADER_START as *const u32,
    revocation_owc: crate::LOADER_REVOCATION_OFFSET,
    function_codes: &[FunctionCode::App as u32, FunctionCode::UpdatedApp as u32],
};

pub const LOADER_TO_SWAP: SecurityConfiguration = SecurityConfiguration {
    image_ptr: (crate::offsets::baosec::SWAP_HEADER_LEN - crate::signatures::SIGBLOCK_LEN) as *const u32,
    pubkey_ptr: crate::LOADER_START as *const u32,
    revocation_owc: crate::LOADER_REVOCATION_OFFSET,
    function_codes: &[FunctionCode::Swap as u32, FunctionCode::UpdatedSwap as u32],
};
