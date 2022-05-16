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

#[cfg(not(any(target_os = "none", target_os = "xous")))]
use core::sync::atomic::AtomicU64;

// Secretly, you can change this by setting the XOUS_SEED environment variable.
// I don't lke environment variables because where do you document features like this?
// But, this was the most expedient way to get all the threads in Hosted mode to pick up a seed.
// The code that reads the varable this is all the way over in xous-rs\src\arch\hosted\mod.rs#29, and
// it's glommed onto some other static process initialization code because I don't fully understand
// what's going on over there.
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub static TESTING_RNG_SEED: AtomicU64 = AtomicU64::new(0);

pub mod exceptions;
pub use exceptions::*;

pub mod messages;
pub use messages::*;

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
    pub const fn from_u32(a0: u32, a1: u32, a2: u32, a3: u32) -> SID {
        SID([a0, a1, a2, a3])
    }
    pub const fn from_array(a: [u32; 4]) -> SID {
        SID(a)
    }
    pub const fn to_u32(&self) -> (u32, u32, u32, u32) {
        (self.0[0], self.0[1], self.0[2], self.0[3])
    }
    pub const fn to_array(&self) -> [u32; 4] {
        self.0
    }
}

impl core::str::FromStr for SID {
    type Err = ();

    fn from_str(s: &str) -> core::result::Result<SID, ()> {
        Self::from_bytes(s.as_bytes()).ok_or(())
    }
}

impl From<[u32; 4]> for SID {
    fn from(src: [u32; 4]) -> Self {
        Self::from_u32(src[0], src[1], src[2], src[3])
    }
}

impl From<&[u32; 4]> for SID {
    fn from(src: &[u32; 4]) -> Self {
        Self::from_array(*src)
    }
}

impl From<SID> for [u32; 4] {
    fn from(s: SID) -> [u32; 4] {
        s.0
    }
}

/// Connection ID
pub type CID = u32;

/// Thread ID
pub type TID = usize;

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct MemoryRange {
    pub(crate) addr: MemoryAddress,
    pub(crate) size: MemorySize,
}

#[cfg(feature = "bitflags")]
bitflags! {
    /// Flags to be passed to the MapMemory struct.
    /// Note that it is an error to have memory be
    /// writable and not readable.
    pub struct MemoryFlags: usize {
        /// Free this memory
        const FREE      = 0b0000_0000;

        /// Immediately allocate this memory.  Otherwise it will
        /// be demand-paged.  This is implicitly set when `phys`
        /// is not 0.
        const RESERVE   = 0b0000_0001;

        /// Allow the CPU to read from this page.
        const R         = 0b0000_0010;

        /// Allow the CPU to write to this page.
        const W         = 0b0000_0100;

        /// Allow the CPU to execute from this page.
        const X         = 0b0000_1000;
    }
}
#[cfg(feature = "bitflags")]
pub(crate) fn get_bits(bf: &MemoryFlags) -> usize {
    bf.bits()
}
#[cfg(feature = "bitflags")]
pub(crate) fn from_bits(raw: usize) -> Option<MemoryFlags> {
    MemoryFlags::from_bits(raw)
}

#[cfg(not(feature = "bitflags"))]
pub type MemoryFlags = usize;
#[cfg(not(feature = "bitflags"))]
pub(crate) fn get_bits(bf: &MemoryFlags) -> usize {
    *bf
}
#[cfg(not(feature = "bitflags"))]
pub(crate) fn from_bits(raw: usize) -> Option<MemoryFlags> {
    Some(raw)
}

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
            UnknownError => usize::MAX,
        }
    }
}

#[repr(C)]
pub struct Context {
    stack: StackPointer,
    pid: PID,
}

impl MemoryRange {
    /// # Safety
    ///
    /// This allows for creating a `MemoryRange` from any arbitrary pointer,
    /// so it is imperitive that this only be used to point to valid, page-aligned
    /// ranges.
    pub unsafe fn new(addr: usize, size: usize) -> core::result::Result<MemoryRange, Error> {
        assert!(
            addr != 0,
            "tried to construct a memory range with a null pointer"
        );
        assert!(size != 0, "tried to construct a zero-length memory range");
        Ok(MemoryRange {
            addr: MemoryAddress::new(addr).ok_or(Error::BadAddress)?,
            size: MemorySize::new(size).ok_or(Error::BadAddress)?,
        })
    }

    #[deprecated(since = "0.8.4", note = "Please use `new(addr, size)` instead")]
    pub fn from_parts(addr: MemoryAddress, size: MemorySize) -> MemoryRange {
        MemoryRange { addr, size }
    }

    pub fn len(&self) -> usize {
        self.size.get()
    }

    pub fn is_empty(&self) -> bool {
        self.size.get() > 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.addr.get() as *const u8
    }

    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.addr.get() as *mut u8
    }

    /// Return this memory as a slice of values. The resulting slice
    /// will cover the maximum number of elements given the size of `T`.
    /// For example, if the allocation is 4096 bytes, then the resulting
    /// `&[u8]` would have 4096 elements, `&[u16]` would have 2048, and
    /// `&[u32]` would have 1024. Values are rounded down.
    pub fn as_slice<T>(&self) -> &[T] {
        // This is safe because the pointer and length are guaranteed to
        // be valid, as long as the user hasn't already called `as_ptr()`
        // and done something unsound with the resulting pointer.
        unsafe {
            core::slice::from_raw_parts(
                self.as_ptr() as *const T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }

    /// Return this memory as a slice of mutable values. The resulting slice
    /// will cover the maximum number of elements given the size of `T`.
    /// For example, if the allocation is 4096 bytes, then the resulting
    /// `&[u8]` would have 4096 elements, `&[u16]` would have 2048, and
    /// `&[u32]` would have 1024. Values are rounded down.
    pub fn as_slice_mut<T>(&mut self) -> &mut [T] {
        // This is safe because the pointer and length are guaranteed to
        // be valid, as long as the user hasn't already called `as_ptr()`
        // and done something unsound with the resulting pointer.
        unsafe {
            core::slice::from_raw_parts_mut(
                self.as_mut_ptr() as *mut T,
                self.len() / core::mem::size_of::<T>(),
            )
        }
    }
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
    Ok,
    Error(Error),
    MemoryAddress(MemoryAddress),
    MemoryRange(MemoryRange),
    ReadyThreads(
        usize, /* count */
        usize,
        /* pid0 */ usize, /* context0 */
        usize,
        /* pid1 */ usize, /* context1 */
        usize,
        /* pid2 */ usize, /* context2 */
    ),
    ResumeProcess,
    ServerID(SID),
    ConnectionID(CID),
    NewServerID(SID, CID),
    Message(MessageEnvelope),
    ThreadID(TID),
    ProcessID(PID),

    /// The requested system call is unimplemented
    Unimplemented,

    /// The process is blocked and should perform the read() again. This is only
    /// ever seen in `Hosted` mode, because when running natively the kernel
    /// simply never schedules the process.
    BlockedProcess,

    /// A scalar with one value
    Scalar1(usize),

    /// A scalar with two values
    Scalar2(usize, usize),

    /// The syscall should be attempted again. This is returned when calling
    /// functions such as `try_connect()` and `try_send()` that may block.
    RetryCall,

    /// The message was successful but no value was returned.
    None,

    /// Memory was returned, and more information is available.
    MemoryReturned(
        Option<MemorySize>, /* offset */
        Option<MemorySize>, /* valid */
    ),

    UnknownResult(usize, usize, usize, usize, usize, usize, usize),
}

impl Result {
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
            Result::Message(me) => {
                let me_enc = me.to_usize();
                [
                    8, me_enc[0], me_enc[1], me_enc[2], me_enc[3], me_enc[4], me_enc[5], me_enc[6],
                ]
            }
            Result::ThreadID(ctx) => [9, *ctx as usize, 0, 0, 0, 0, 0, 0],
            Result::ProcessID(pid) => [10, pid.get() as _, 0, 0, 0, 0, 0, 0],
            Result::Unimplemented => [11, 0, 0, 0, 0, 0, 0, 0],
            Result::BlockedProcess => [12, 0, 0, 0, 0, 0, 0, 0],
            Result::Scalar1(a) => [13, *a, 0, 0, 0, 0, 0, 0],
            Result::Scalar2(a, b) => [14, *a, *b, 0, 0, 0, 0, 0],
            Result::NewServerID(sid, cid) => {
                let s = sid.to_u32();
                [
                    15,
                    s.0 as _,
                    s.1 as _,
                    s.2 as _,
                    s.3 as _,
                    *cid as usize,
                    0,
                    0,
                ]
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
            6 => Result::ServerID(SID::from_u32(
                src[1] as _,
                src[2] as _,
                src[3] as _,
                src[4] as _,
            )),
            7 => Result::ConnectionID(src[1] as CID),
            8 => {
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
                    3 => Message::Scalar(ScalarMessage::from_usize(
                        src[3], src[4], src[5], src[6], src[7],
                    )),
                    4 => Message::BlockingScalar(ScalarMessage::from_usize(
                        src[3], src[4], src[5], src[6], src[7],
                    )),
                    _ => return Result::Error(Error::InternalError),
                };
                Result::Message(MessageEnvelope {
                    sender: MessageSender::from_usize(sender),
                    body: message,
                })
            }
            9 => Result::ThreadID(src[1] as TID),
            10 => Result::ProcessID(PID::new(src[1] as _).unwrap()),
            11 => Result::Unimplemented,
            12 => Result::BlockedProcess,
            13 => Result::Scalar1(src[1]),
            14 => Result::Scalar2(src[1], src[2]),
            15 => Result::NewServerID(
                SID::from_u32(src[1] as _, src[2] as _, src[3] as _, src[4] as _),
                src[5] as _,
            ),
            16 => Result::RetryCall,
            17 => Result::None,
            18 => Result::MemoryReturned(MemorySize::new(src[1]), MemorySize::new(src[2])),
            _ => Result::UnknownResult(src[0], src[1], src[2], src[3], src[4], src[5], src[6]),
        }
    }

    /// If the Result has memory attached to it, return the memory
    pub fn memory(&self) -> Option<MemoryRange> {
        match self {
            Result::Message(msg) => match &msg.body {
                Message::Move(memory_message)
                | Message::Borrow(memory_message)
                | Message::MutableBorrow(memory_message) => Some(memory_message.buf),
                _ => None,
            },
            _ => None,
        }
    }
}

impl From<Error> for Result {
    fn from(e: Error) -> Self {
        Result::Error(e)
    }
}

pub type SysCallRequest = core::result::Result<crate::syscall::SysCall, Error>;
pub type SysCallResult = core::result::Result<Result, Error>;

#[macro_export]
macro_rules! msg_scalar_unpack {
    // the args are `tt` so that you can specify _ as the arg
    ($msg:ident, $arg1:tt, $arg2:tt, $arg3:tt, $arg4:tt, $body: block) => {{
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
    ($msg:ident, $arg1:tt, $arg2:tt, $arg3:tt, $arg4:tt, $body: block) => {{
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
