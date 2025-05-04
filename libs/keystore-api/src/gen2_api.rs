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
    /// clear a cached password
    ClearPasswordCacheEntry = 7,

    // Gen-2 extensions are at 256 and up
    /// Get a non-secret 64-bit random identifier
    GetDna = 256,
    EnsurePassword = 257,
}
