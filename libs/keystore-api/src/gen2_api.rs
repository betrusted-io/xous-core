pub const SERVER_NAME_KEYS: &str = "_Keystore server_";

/// Size of a checksummed block in pages. 0x100 = 256 pages,
/// or 1 MiB for a checksummed block. This is specified in 4kiB pages
/// because it really doesn't make sense to checksum anything smaller
/// than that, and it allows us to grow the size of a single checksummed
/// block to well over 4GiB.
pub const CHECKSUM_BLOCKLEN_PAGE: u32 = 0x100;
/// TODO: set PDDB length based on board-specific config params
pub const TOTAL_CHECKSUMS: u32 = 4096 * 1024 / (CHECKSUM_BLOCKLEN_PAGE * 4096);

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
    /// initiate an AES oracle operation
    AesOracle = 4,
    /// initiate key wrapper operation
    AesKwp = 5,
    /// Ephemeral secret operations. Split into MSB/LSB pairs, because we want to strictly use
    /// scalar messages only for this. This helps to ensure to leakage of secrets to memory pages
    /// (there is some risk of stack spillage, but this at least reduces the attack surface).
    /// The ephemeral secret is 192 bits long - so the `Scalar` operation is split into 1x control
    /// word, and 3x 32 bit words that transmit the secret.
    Ephemeral = 256,
    /// Flag operations
    GetFlags = 512,
    SetFlags = 513,
    /// One way counter operations
    GetOneWayCounter = 768,
    #[cfg(feature = "owc-inc")]
    IncOneWayCounter = 769,

    /// Application key operations
    #[cfg(feature = "app-keys")]
    AppKeyOp = 1024,

    /// Call to trigger that swap is encrypted
    #[cfg(feature = "swap")]
    EnsureSwapEncryption = 1536,

    // ----- below are non-cryptographic opcodes but used to manipulate sensitive state -----
    /// Set bootwait parameters
    Bootwait = 4096,
    IsDeveloper = 4097,

    /// Used to map unknown opcodes
    InvalidCall = 65535,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum EphemeralOp {
    GetLsb,
    SetLsb,
    GetMsb,
    SetMsb,
}

/// Series of magic numbers not meant for cryptographic authentication,
/// but for detecting fat-fingered API implementations.
pub const OWC_MAGIC_GET: [usize; 3] = [0x2b46_2ab3, 0xf7e3_1b59, 0x7bba_d222];
pub const OWC_MAGIC_INC: [usize; 3] = [0xeb5d_fc81, 0x8b6d_6a90, 0xf720_491a];
pub const APPKEY_GUARD: [u32; 4] = [0x3936_e7a6, 0xfc53_a1f7, 0xf2eb_16f3, 0xd2cc_22d9];
