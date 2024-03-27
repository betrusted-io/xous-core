use core::convert::TryInto;
use core::num::{NonZeroU8, NonZeroUsize};

pub type MemoryAddress = NonZeroUsize;
pub type MemorySize = NonZeroUsize;
pub type StackPointer = usize;

pub type PID = NonZeroU8;
pub type Connection = usize;

pub const MAX_CID: usize = 34;

pub const FLASH_PHYS_BASE: u32 = 0x2000_0000;
pub const SOC_REGION_LOC: u32 = 0x0000_0000;
pub const SOC_REGION_LEN: u32 = 0x00D0_0000; // gw + staging + loader + kernel

// note to self: if these locations change, be sure to update the "filters" addresses
// in the gateware, so that we are consistent on what parts of the SPINOR are allowed access via USB debug
pub const SOC_MAIN_GW_LOC: u32 = 0x0000_0000; // gateware - primary loading address
pub const SOC_MAIN_GW_LEN: u32 = 0x0028_0000;
pub const SOC_STAGING_GW_LOC: u32 = 0x0028_0000; // gateware - staging copy
pub const SOC_STAGING_GW_LEN: u32 = 0x0028_0000;

pub const LOADER_LOC: u32 = 0x0050_0000; // loader - base
pub const LOADER_CODE_LEN: u32 = 0x0003_0000; // code region only
pub const LOADER_FONT_LOC: u32 = 0x0053_0000; // should be the same as graphics-server/src/fontmap.rs/FONT_BASE
pub const LOADER_FONT_LEN: u32 = 0x0044_0000; // length of font region only
pub const LOADER_TOTAL_LEN: u32 = LOADER_CODE_LEN + LOADER_FONT_LEN; // code + font

pub const EARLY_SETTINGS: u32 = 0x0097_0000;

pub const KERNEL_LOC: u32 = 0x0098_0000; // kernel start
pub const KERNEL_LEN: u32 = 0x0140_0000; // max kernel length = 0xA0_0000 * 2 => half the area for backup kernel & updates
pub const KERNEL_BACKUP_OFFSET: u32 = KERNEL_LEN - 0x1000; // last page of kernel is where the backup block gets located = 0x1D7_F000

pub const EC_REGION_LOC: u32 = 0x07F8_0000; // EC update staging area. Must be aligned to a 64k-address.
pub const EC_WF200_PKG_LOC: u32 = 0x07F8_0000;
pub const EC_WF200_PKG_LEN: u32 = 0x0004_E000;
pub const EC_FW_PKG_LOC: u32 = 0x07FC_E000;
pub const EC_FW_PKG_LEN: u32 = 0x0003_2000;
pub const EC_REGION_LEN: u32 = 0x0008_0000;

pub const PDDB_LOC: u32 = 0x01D8_0000; // PDDB start
pub const PDDB_LEN: u32 = EC_REGION_LOC - PDDB_LOC; // must be 64k-aligned (bulk erase block size) for proper function.

// quantum alloted to each process before a context switch is forced
pub const BASE_QUANTA_MS: u32 = 10;

// sentinel used by test infrastructure to assist with parsing
// The format of any test infrastructure output to recover is as follows:
// _|TT|_<ident>,<data separated by commas>,_|TE|_
// where _|TT|_ and _|TE|_ are bookends around the data to be reported
// <ident> is a single-word identifier that routes the data to a given parser
// <data> is free-form data, which will be split at comma boundaries by the parser
pub const BOOKEND_START: &str = "_|TT|_";
pub const BOOKEND_END: &str = "_|TE|_";

/// Hard-wired PID of the swapper
#[cfg(feature = "swap")]
pub const SWAPPER_PID: u8 = 2;

#[cfg(not(any(target_os = "xous", target_os = "none")))]
use core::sync::atomic::AtomicU64;

// Secretly, you can change this by setting the XOUS_SEED environment variable.
// I don't lke environment variables because where do you document features like this?
// But, this was the most expedient way to get all the threads in Hosted mode to pick up a seed.
// The code that reads the varable this is all the way over in xous-rs\src\arch\hosted\mod.rs#29, and
// it's glommed onto some other static process initialization code because I don't fully understand
// what's going on over there.
#[cfg(not(any(target_os = "xous", target_os = "none")))]
pub static TESTING_RNG_SEED: AtomicU64 = AtomicU64::new(0);

pub mod exceptions;
pub use exceptions::*;

pub mod memoryflags;
pub use memoryflags::*;

pub mod memoryrange;
pub use memoryrange::*;

pub mod messages;
pub use messages::*;

pub mod limits;
pub use limits::*;

use crate::arch::ProcessStartup;

/// Server ID
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SID([u32; 4]);
impl SID {
    pub fn from_bytes(b: &[u8]) -> Option<SID> {
        if b.len() > 16 {
            None
        } else {
            let mut sid = [0; 4];
            let mut byte_iter = b.chunks_exact(4);
            if let Some(val) = byte_iter.next() {
                sid[0] = u32::from_le_bytes(val.try_into().ok()?);
            }
            if let Some(val) = byte_iter.next() {
                sid[1] = u32::from_le_bytes(val.try_into().ok()?);
            }
            if let Some(val) = byte_iter.next() {
                sid[2] = u32::from_le_bytes(val.try_into().ok()?);
            }
            if let Some(val) = byte_iter.next() {
                sid[3] = u32::from_le_bytes(val.try_into().ok()?);
            }
            Some(SID(sid))
        }
    }

    pub const fn from_u32(a0: u32, a1: u32, a2: u32, a3: u32) -> SID { SID([a0, a1, a2, a3]) }

    pub const fn from_array(a: [u32; 4]) -> SID { SID(a) }

    pub const fn to_u32(&self) -> (u32, u32, u32, u32) { (self.0[0], self.0[1], self.0[2], self.0[3]) }

    pub const fn to_array(&self) -> [u32; 4] { self.0 }
}

impl core::str::FromStr for SID {
    type Err = ();

    fn from_str(s: &str) -> core::result::Result<SID, ()> { Self::from_bytes(s.as_bytes()).ok_or(()) }
}

impl From<[u32; 4]> for SID {
    fn from(src: [u32; 4]) -> Self { Self::from_u32(src[0], src[1], src[2], src[3]) }
}

impl From<&[u32; 4]> for SID {
    fn from(src: &[u32; 4]) -> Self { Self::from_array(*src) }
}

impl From<SID> for [u32; 4] {
    fn from(s: SID) -> [u32; 4] { s.0 }
}

/// Connection ID
pub type CID = u32;

/// Thread ID
pub type TID = usize;

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

pub fn pid_from_usize(src: usize) -> core::result::Result<PID, Error> {
    if src > u8::MAX as _ {
        return Err(Error::InvalidPID);
    }
    PID::new(src as u8).ok_or(Error::InvalidPID)
}

#[repr(usize)]
#[derive(Debug, PartialEq)]
pub enum Error {
    NoError = 0,
    BadAlignment = 1,
    BadAddress = 2,
    OutOfMemory = 3,
    MemoryInUse = 4,
    InterruptNotFound = 5,
    InterruptInUse = 6,
    InvalidString = 7,
    ServerExists = 8,
    ServerNotFound = 9,
    ProcessNotFound = 10,
    ProcessNotChild = 11,
    ProcessTerminated = 12,
    Timeout = 13,
    InternalError = 14,
    ServerQueueFull = 15,
    ThreadNotAvailable = 16,
    UnhandledSyscall = 17,
    InvalidSyscall = 18,
    ShareViolation = 19,
    InvalidThread = 20,
    InvalidPID = 21,
    UnknownError = 22,
    AccessDenied = 23,
    UseBeforeInit = 24,
    DoubleFree = 25,
    DebugInProgress = 26,
    InvalidLimit = 27,
}

impl Error {
    pub fn from_usize(arg: usize) -> Self {
        use crate::Error::*;
        match arg {
            0 => NoError,
            1 => BadAlignment,
            2 => BadAddress,
            3 => OutOfMemory,
            4 => MemoryInUse,
            5 => InterruptNotFound,
            6 => InterruptInUse,
            7 => InvalidString,
            8 => ServerExists,
            9 => ServerNotFound,
            10 => ProcessNotFound,
            11 => ProcessNotChild,
            12 => ProcessTerminated,
            13 => Timeout,
            14 => InternalError,
            15 => ServerQueueFull,
            16 => ThreadNotAvailable,
            17 => UnhandledSyscall,
            18 => InvalidSyscall,
            19 => ShareViolation,
            20 => InvalidThread,
            21 => InvalidPID,
            23 => AccessDenied,
            24 => UseBeforeInit,
            25 => DoubleFree,
            26 => DebugInProgress,
            27 => InvalidLimit,
            _ => UnknownError,
        }
    }

    pub fn to_usize(&self) -> usize {
        use crate::Error::*;
        match *self {
            NoError => 0,
            BadAlignment => 1,
            BadAddress => 2,
            OutOfMemory => 3,
            MemoryInUse => 4,
            InterruptNotFound => 5,
            InterruptInUse => 6,
            InvalidString => 7,
            ServerExists => 8,
            ServerNotFound => 9,
            ProcessNotFound => 10,
            ProcessNotChild => 11,
            ProcessTerminated => 12,
            Timeout => 13,
            InternalError => 14,
            ServerQueueFull => 15,
            ThreadNotAvailable => 16,
            UnhandledSyscall => 17,
            InvalidSyscall => 18,
            ShareViolation => 19,
            InvalidThread => 20,
            InvalidPID => 21,
            AccessDenied => 23,
            UseBeforeInit => 24,
            DoubleFree => 25,
            DebugInProgress => 26,
            InvalidLimit => 27,
            UnknownError => usize::MAX,
        }
    }
}

#[repr(C)]
pub struct Context {
    stack: StackPointer,
    pid: PID,
}

/// Which memory region the operation should affect.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum MemoryType {
    /// The address where addresses go when no `virt` is specified.
    Default = 1,

    /// Addresses will begin here when `IncreaseHeap` is called.
    Heap = 2,

    /// When messages are passed to a process, they will go here.
    Messages = 3,

    /// Unlike other memory types, this defines the "end" of the region.
    Stack = 4,
}

impl From<usize> for MemoryType {
    fn from(arg: usize) -> Self {
        match arg {
            2 => MemoryType::Heap,
            3 => MemoryType::Messages,
            4 => MemoryType::Stack,
            _ => MemoryType::Default,
        }
    }
}

impl core::fmt::Display for MemoryType {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            MemoryType::Default => write!(fmt, "Default"),
            MemoryType::Heap => write!(fmt, "Heap"),
            MemoryType::Stack => write!(fmt, "Stack"),
            MemoryType::Messages => write!(fmt, "Messages"),
        }
    }
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum Result {
    // 0
    Ok,
    // 1
    Error(Error),

    // 2
    MemoryAddress(MemoryAddress),

    // 3
    MemoryRange(MemoryRange),

    // 4
    ReadyThreads(
        usize, /* count */
        usize,
        /* pid0 */ usize, /* context0 */
        usize,
        /* pid1 */ usize, /* context1 */
        usize,
        /* pid2 */ usize, /* context2 */
    ),

    // 5
    ResumeProcess,

    // 6
    ServerID(SID),

    // 7
    ConnectionID(CID),

    // 8
    NewServerID(SID, CID),

    // 9
    MessageEnvelope(MessageEnvelope),

    // 10
    ThreadID(TID),

    // 11
    ProcessID(PID),

    /// 12: The requested system call is unimplemented
    Unimplemented,

    /// 13: The process is blocked and should perform the read() again. This is only
    /// ever seen in `Hosted` mode, because when running natively the kernel
    /// simply never schedules the process.
    BlockedProcess,

    /// 14: A scalar with one value
    Scalar1(usize),

    /// 15: A scalar with two values
    Scalar2(usize, usize),

    /// 16: The syscall should be attempted again. This is returned when calling
    /// functions such as `try_connect()` and `try_send()` that may block.
    RetryCall,

    /// The message was successful but no value was returned.
    None,

    /// Memory was returned, and more information is available.
    MemoryReturned(Option<MemorySize> /* offset */, Option<MemorySize> /* valid */),

    /// Returned when a process has started. This describes the new process to
    /// the caller.
    NewProcess(ProcessStartup),

    /// 20: A scalar with five values
    Scalar5(usize, usize, usize, usize, usize),

    // 21: A message is returned as part of `send_message()` when the result is blocking
    Message(Message),

    UnknownResult(usize, usize, usize, usize, usize, usize, usize),
}

impl Result {
    fn add_opcode(opcode: usize, args: [usize; 7]) -> [usize; 8] {
        [opcode, args[0], args[1], args[2], args[3], args[4], args[5], args[6]]
    }

    pub fn to_args(&self) -> [usize; 8] {
        match self {
            Result::Ok => [0, 0, 0, 0, 0, 0, 0, 0],
            Result::Error(e) => [1, e.to_usize(), 0, 0, 0, 0, 0, 0],
            Result::MemoryAddress(s) => [2, s.get(), 0, 0, 0, 0, 0, 0],
            Result::MemoryRange(r) => [3, r.addr.get(), r.size.get(), 0, 0, 0, 0, 0],
            Result::ReadyThreads(count, pid0, ctx0, pid1, ctx1, pid2, ctx2) => {
                [4, *count, *pid0, *ctx0, *pid1, *ctx1, *pid2, *ctx2]
            }
            Result::ResumeProcess => [5, 0, 0, 0, 0, 0, 0, 0],
            Result::ServerID(sid) => {
                let s = sid.to_u32();
                [6, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            Result::ConnectionID(cid) => [7, *cid as usize, 0, 0, 0, 0, 0, 0],
            Result::MessageEnvelope(me) => {
                let me_enc = me.to_usize();
                [9, me_enc[0], me_enc[1], me_enc[2], me_enc[3], me_enc[4], me_enc[5], me_enc[6]]
            }
            Result::ThreadID(ctx) => [10, *ctx, 0, 0, 0, 0, 0, 0],
            Result::ProcessID(pid) => [11, pid.get() as _, 0, 0, 0, 0, 0, 0],
            Result::Unimplemented => [21, 0, 0, 0, 0, 0, 0, 0],
            Result::BlockedProcess => [13, 0, 0, 0, 0, 0, 0, 0],
            Result::Scalar1(a) => [14, *a, 0, 0, 0, 0, 0, 0],
            Result::Scalar2(a, b) => [15, *a, *b, 0, 0, 0, 0, 0],
            Result::NewServerID(sid, cid) => {
                let s = sid.to_u32();
                [8, s.0 as _, s.1 as _, s.2 as _, s.3 as _, *cid as usize, 0, 0]
            }
            Result::RetryCall => [16, 0, 0, 0, 0, 0, 0, 0],
            Result::None => [17, 0, 0, 0, 0, 0, 0, 0],
            Result::MemoryReturned(offset, valid) => [
                18,
                offset.map(|o| o.get()).unwrap_or_default(),
                valid.map(|v| v.get()).unwrap_or_default(),
                0,
                0,
                0,
                0,
                0,
            ],
            Result::NewProcess(p) => Self::add_opcode(19, p.into()),
            Result::Scalar5(a, b, c, d, e) => [20, *a, *b, *c, *d, *e, 0, 0],
            Result::Message(message) => {
                let encoded = message.to_usize();
                [21, encoded[0], encoded[1], encoded[2], encoded[3], encoded[4], encoded[5], 0]
            }
            Result::UnknownResult(arg1, arg2, arg3, arg4, arg5, arg6, arg7) => {
                [usize::MAX, *arg1, *arg2, *arg3, *arg4, *arg5, *arg6, *arg7]
            }
        }
    }

    pub fn from_args(src: [usize; 8]) -> Self {
        match src[0] {
            0 => Result::Ok,
            1 => Result::Error(Error::from_usize(src[1])),
            2 => match MemoryAddress::new(src[1]) {
                None => Result::Error(Error::InternalError),
                Some(s) => Result::MemoryAddress(s),
            },
            3 => {
                let addr = match MemoryAddress::new(src[1]) {
                    None => return Result::Error(Error::InternalError),
                    Some(s) => s,
                };
                let size = match MemorySize::new(src[2]) {
                    None => return Result::Error(Error::InternalError),
                    Some(s) => s,
                };

                Result::MemoryRange(MemoryRange { addr, size })
            }
            4 => Result::ReadyThreads(src[1], src[2], src[3], src[4], src[5], src[6], src[7]),
            5 => Result::ResumeProcess,
            6 => Result::ServerID(SID::from_u32(src[1] as _, src[2] as _, src[3] as _, src[4] as _)),
            7 => Result::ConnectionID(src[1] as CID),
            8 => Result::NewServerID(
                SID::from_u32(src[1] as _, src[2] as _, src[3] as _, src[4] as _),
                src[5] as _,
            ),
            9 => {
                let sender = src[1];
                let message = match src[2] {
                    0 => match MemoryMessage::from_usize(src[3], src[4], src[5], src[6], src[7]) {
                        None => return Result::Error(Error::InternalError),
                        Some(s) => Message::MutableBorrow(s),
                    },
                    1 => match MemoryMessage::from_usize(src[3], src[4], src[5], src[6], src[7]) {
                        None => return Result::Error(Error::InternalError),
                        Some(s) => Message::Borrow(s),
                    },
                    2 => match MemoryMessage::from_usize(src[3], src[4], src[5], src[6], src[7]) {
                        None => return Result::Error(Error::InternalError),
                        Some(s) => Message::Move(s),
                    },
                    3 => Message::Scalar(ScalarMessage::from_usize(src[3], src[4], src[5], src[6], src[7])),
                    4 => Message::BlockingScalar(ScalarMessage::from_usize(
                        src[3], src[4], src[5], src[6], src[7],
                    )),
                    _ => return Result::Error(Error::InternalError),
                };
                Result::MessageEnvelope(MessageEnvelope {
                    sender: MessageSender::from_usize(sender),
                    body: message,
                })
            }
            10 => Result::ThreadID(src[1] as TID),
            11 => Result::ProcessID(PID::new(src[1] as _).unwrap()),
            12 => Result::Unimplemented,
            13 => Result::BlockedProcess,
            14 => Result::Scalar1(src[1]),
            15 => Result::Scalar2(src[1], src[2]),
            16 => Result::RetryCall,
            17 => Result::None,
            18 => Result::MemoryReturned(MemorySize::new(src[1]), MemorySize::new(src[2])),
            19 => Result::NewProcess(src.into()),
            20 => Result::Scalar5(src[1], src[2], src[3], src[4], src[5]),
            21 => Result::Message(match src[1] {
                0 => match MemoryMessage::from_usize(src[2], src[3], src[4], src[5], src[6]) {
                    None => return Result::Error(Error::InternalError),
                    Some(s) => Message::MutableBorrow(s),
                },
                1 => match MemoryMessage::from_usize(src[2], src[3], src[4], src[5], src[6]) {
                    None => return Result::Error(Error::InternalError),
                    Some(s) => Message::Borrow(s),
                },
                2 => match MemoryMessage::from_usize(src[2], src[3], src[4], src[5], src[6]) {
                    None => return Result::Error(Error::InternalError),
                    Some(s) => Message::Move(s),
                },
                3 => Message::Scalar(ScalarMessage::from_usize(src[2], src[3], src[4], src[5], src[6])),
                4 => {
                    Message::BlockingScalar(ScalarMessage::from_usize(src[2], src[3], src[4], src[5], src[6]))
                }
                _ => return Result::Error(Error::InternalError),
            }),
            _ => Result::UnknownResult(src[0], src[1], src[2], src[3], src[4], src[5], src[6]),
        }
    }

    /// If the Result has memory attached to it, return the memory
    pub fn memory(&self) -> Option<&MemoryRange> {
        match self {
            Result::MessageEnvelope(msg) => msg.body.memory(),
            Result::Message(msg) => msg.memory(),
            _ => None,
        }
    }
}

impl From<Error> for Result {
    fn from(e: Error) -> Self { Result::Error(e) }
}

pub type SysCallRequest = core::result::Result<crate::syscall::SysCall, Error>;
pub type SysCallResult = core::result::Result<Result, Error>;

#[macro_export]
macro_rules! msg_scalar_unpack {
    // the args are `tt` so that you can specify _ as the arg
    ($msg:ident, $arg1:tt, $arg2:tt, $arg3:tt, $arg4:tt, $body:block) => {{
        if let xous::Message::Scalar(xous::ScalarMessage {
            id: _,
            arg1: $arg1,
            arg2: $arg2,
            arg3: $arg3,
            arg4: $arg4,
        }) = $msg.body
        {
            $body
        } else {
            log::error!("message expansion failed in msg_scalar_unpack macro")
        }
    }};
}

#[macro_export]
macro_rules! msg_blocking_scalar_unpack {
    // the args are `tt` so that you can specify _ as the arg
    ($msg:ident, $arg1:tt, $arg2:tt, $arg3:tt, $arg4:tt, $body:block) => {{
        if let xous::Message::BlockingScalar(xous::ScalarMessage {
            id: _,
            arg1: $arg1,
            arg2: $arg2,
            arg3: $arg3,
            arg4: $arg4,
        }) = $msg.body
        {
            $body
        } else {
            log::error!("message expansion failed in msg_scalar_unpack macro")
        }
    }};
}

#[cfg(feature = "swap")]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AllocAdvice {
    /// the PID of the allocation, virtual address in PID space, physical address
    Allocate(PID, usize, usize),
    /// the PID of the page freed, virtuall address in PID space, physical address
    Free(PID, usize, usize),
    /// not yet initialized record
    Uninit,
}
#[cfg(feature = "swap")]
impl AllocAdvice {
    pub fn serialize(&self) -> (usize, usize) {
        match self {
            AllocAdvice::Allocate(pid, vaddr, paddr) => {
                (
                    (pid.get() as usize) << 24 | (vaddr >> 12),
                    (1 << 24) | (paddr >> 12), // 1 indicates an alloc
                )
            }
            AllocAdvice::Free(pid, vaddr, paddr) => {
                (
                    (pid.get() as usize) << 24 | (vaddr >> 12),
                    (0 << 24) | (paddr >> 12), // 0 indicates a free
                )
            }
            AllocAdvice::Uninit => (0, 0),
        }
    }

    pub fn deserialize(a0: usize, a1: usize) -> Self {
        if a0 == 0 && a1 == 0 {
            AllocAdvice::Uninit
        } else if (a1 & (1 << 24)) == 0 {
            // don't have to mask a0 or a1 high bits because << 12 shifts the high flag byte out
            AllocAdvice::Free(NonZeroU8::new((a0 >> 24) as u8).unwrap(), a0 << 12, a1 << 12)
        } else {
            AllocAdvice::Allocate(NonZeroU8::new((a0 >> 24) as u8).unwrap(), a0 << 12, a1 << 12)
        }
    }
}
