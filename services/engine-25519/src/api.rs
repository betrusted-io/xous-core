pub(crate) const SERVER_NAME_ENGINE25519: &str     = "_Curve-25519 Accelerator Engine_";

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub (crate) struct Job {
    /// unique identifier so the server knows who the request is coming from
    id: [u32; 3],
    /// start location for microcode load
    uc_start: u32,
    /// length of the microcode to run
    uc_len: u32,
    /// microcode program
    ucode: [u32; 1024],
    /// initial register file contents (contains any arguments to the program)
    rf: [u32; 64],
    /// which register window, if any, to use for the job
    window: Option<u8>,
}

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

    /// Runs a job
    RunJob,

    /// a function that can be polled to determine if the block has been currently acquired
    IsIdle,

    /// IRQ handler feedback
    EngineDone,
    IllegalOpcode,

    /// exit the server
    Quit,

    // note that suspend/resume is handled by a secondary thread that can concurrently
    // interrupt the main thread and store the state
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum JobResult {
    /// returns a copy of the entire register file as a result
    Result([u32; 64]),
    SuspendError,
    Uninitialized,
    IdMismatch,
}