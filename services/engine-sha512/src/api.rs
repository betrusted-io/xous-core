#[allow(dead_code)]
pub(crate) const SERVER_NAME_SHA512: &str = "_Sha512 hardware accelerator server_"; // not used in hosted config
mod rkyv_enum;
pub use rkyv_enum::*;

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct Sha2Finalize {
    pub id: [u32; 3],
    pub result: Sha2Result,
    pub length_in_bits: Option<u64>,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug, Copy, Clone)]
pub(crate) enum Sha2Config {
    Sha512,
    Sha512Trunc256,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct Sha2Update {
    pub id: [u32; 3], // our unique identifier so the server knows who the request is coming from
    pub buffer: [u8; 3968], // leave one SHA chunk-sized space for overhead, so the whole message fits in one page of memory
    pub len: u16,           // length of just this buffer, fits in 16 bits
}

#[allow(dead_code)]
#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Acquire an exclusive lock on the hardware
    /// sends a 96-bit random key + config word, returns true or false if acquisition was successful
    /// note: 96-bit space has a pcollision=10^-18 for 400,000 concurrent hash requests,
    /// pcollision=10^-15 for 13,000,000 concurrent hash requests.
    /// for context, a typical consumer SSD has an uncorrectable bit error rate of 10^-15,
    /// and we probably expect about 3-4 concurrent hash requests in the worst case.
    /// Acquisition will always fail if a Suspend request is pending.
    AcquireExclusive,

    /// Used by higher level coordination processes to acquire a lock on the hardware unit
    /// to prevent any new transactions from occuring. The lock is automatically cleared on
    /// a resume, or by an explicit release
    AcquireSuspendLock,
    /// this is to be used if we decided in the end we aren't going to suspend.
    AbortSuspendLock,

    /// sends a buffer of [u8] for updating the hash
    /// This function will fail if the hardware was shut down with a suspend/resume while hashing
    Update,

    /// finalizes a hash, but exclusive lock is kept. Return value is the requested hash.
    /// This function will fail if the hardware was shut down with a suspend/resume while hashing
    Finalize,

    /// drops the lock on hardware, resets state
    /// finalize and reset are split to maintain API compatibility with the Digest API
    Reset,

    /// a function that can be polled to determine if the block has been currently acquired
    IsIdle,

    /// exit the server
    Quit,
    #[cfg(feature = "event_wait")]
    IrqEvent,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum SusResOps {
    /// Suspend/resume callback
    SuspendResume,
    /// exit the thread
    Quit,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum FallbackStrategy {
    HardwareThenSoftware,
    WaitForHardware,
    SoftwareOnly,
}
