pub(crate) const SERVER_NAME_ENGINE25519: &str = "_Curve-25519 Accelerator Engine_";

// I don't understand why clippy says these are unused. But clippy is wrong. Shut up, clippy.
#[allow(dead_code)]
pub(crate) const UCODE_U8_BASE: usize = 0x0;
#[allow(dead_code)]
pub(crate) const UCODE_U32_BASE: usize = 0x0;
#[allow(dead_code)]
pub(crate) const UCODE_U32_SIZE: usize = 0x1000 / 4;
#[allow(dead_code)]
pub(crate) const UCODE_U8_SIZE: usize = 0x1000;
#[allow(dead_code)]
pub(crate) const RF_U8_BASE: usize = 0x1_0000;
#[allow(dead_code)]
pub(crate) const RF_U32_BASE: usize = 0x1_0000 / 4;
#[allow(dead_code)]
pub(crate) const RF_TOTAL_U32_SIZE: usize = 0x4000 / 4;
#[allow(dead_code)]
pub(crate) const RF_TOTAL_U8_SIZE: usize = 0x4000;

pub(crate) const NUM_REGS: usize = 32;
pub(crate) const BITWIDTH: usize = 256;
#[allow(dead_code)] // not used in hosted
pub(crate) const NUM_WINDOWS: usize = 16;
pub const RF_SIZE_IN_U32: usize = NUM_REGS * (BITWIDTH / 32); // 32 registers, 256 bits/register/32 bits per u32
#[allow(dead_code)] // not used in hosted
pub const TOTAL_RF_SIZE_IN_U32: usize = NUM_REGS * (BITWIDTH / 32) * NUM_WINDOWS; // 32 registers, 256 bits/register/32 bits per u32, times 16 windows

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub struct Job {
    /// If present the SID of the server to which we should return results asynchronously.
    /// If None, then the job will run synchronously.
    pub id: Option<[u32; 4]>,
    /// start location for microcode load
    pub uc_start: u32,
    /// length of the microcode to run
    pub uc_len: u32,
    /// microcode program
    pub ucode: [u32; 1024],
    /// initial register file contents (also contains any arguments to the program)
    pub rf: [u32; RF_SIZE_IN_U32],
    /// which register window, if any, to use for the job
    pub window: Option<u8>,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, Copy)]
pub struct MontgomeryJob {
    pub x0_u: [u8; 32],
    pub x0_w: [u8; 32],
    pub x1_u: [u8; 32],
    pub x1_w: [u8; 32],
    pub affine_u: [u8; 32],
    pub scalar: [u8; 32],
}

mod rkyv_enum;
pub use rkyv_enum::*;

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Runs a job, if the server is not already occupied
    RunJob,

    /// MontgomeryJob
    MontgomeryJob,

    /// a function that can be polled to determine if the block has been currently acquired
    IsFree,

    /// IRQ handler feedback
    EngineDone,
    IllegalOpcode,

    // note: suspend/resume handled by a separate thread and server
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

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum SusResOps {
    /// Suspend/resume callback
    SuspendResume,
    /// exit the thread
    Quit,
}
