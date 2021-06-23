pub(crate) const SERVER_NAME_ENGINE25519: &str     = "_Curve-25519 Accelerator Engine_";

pub(crate) const NUM_REGS: usize = 32;
pub(crate) const BITWIDTH: usize = 256;
pub(crate) const RF_SIZE_IN_U32: usize = NUM_REGS*(BITWIDTH/core::mem::size_of(u32)); // 32 registers, 256 bits/register, divided by 4 bytes per u32

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub (crate) struct Job {
    /// the SID of the server to which we should return results. Ideally, this is an application-specific server, and not your main loop server.
    id: [u32; 4],
    /// start location for microcode load
    uc_start: u32,
    /// length of the microcode to run
    uc_len: u32,
    /// microcode program
    ucode: [u32; 1024],
    /// initial register file contents (also contains any arguments to the program)
    rf: [u32; RF_SIZE_IN_U32],
    /// which register window, if any, to use for the job
    window: Option<u8>,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Runs a job, if the server is not already occupied
    RunJob,

    /// a function that can be polled to determine if the block has been currently acquired
    IsFree,

    /// IRQ handler feedback
    EngineDone,
    IllegalOpcode,

    /// Suspend/resume callback
    SuspendResume,

    /// exit the server
    Quit,

    // note that suspend/resume is handled by a secondary thread that can concurrently
    // interrupt the main thread and store the state
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Return {
    Result,
    Quit,
}


#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum JobResult {
    /// returns a copy of the entire register file as a result
    Result([u32; RF_SIZE_IN_U32]),
    Started,
    EngineUnavailable,
    IllegalOpcodeException,
    SuspendError,
}
