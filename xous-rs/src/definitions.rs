use core::num::{NonZeroU8, NonZeroUsize};

pub type MemoryAddress = NonZeroUsize;
pub type MemorySize = NonZeroUsize;
pub type StackPointer = usize;
pub type MessageId = usize;

pub type PID = NonZeroU8;
pub type MessageSender = usize;
pub type Connection = usize;

/// Server ID
pub type SID = (u32, u32, u32, u32);

/// Connection ID
pub type CID = usize;

/// Context ID
pub type TID = usize;

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

#[derive(Debug, PartialEq, Copy, Clone)]
pub struct MemoryRange {
    pub addr: MemoryAddress,
    pub size: MemorySize,
}

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

pub fn pid_from_usize(src: usize) -> core::result::Result<PID, Error> {
    if src > u8::MAX as _ {
        return Err(Error::InvalidPID);
    }
    Ok(PID::new(src as u8).ok_or(Error::InvalidPID)?)
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
    InvalidContext = 20,
    InvalidPID = 21,
    UnknownError = 22,
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
            20 => InvalidContext,
            21 => InvalidPID,
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
            InvalidContext => 20,
            InvalidPID => 21,
            UnknownError => usize::MAX,
        }
    }
}

#[repr(C)]
pub struct Context {
    stack: StackPointer,
    pid: PID,
}

#[repr(C)]
#[derive(Debug, PartialEq)]
/// A struct describing memory that is passed between processes.
/// The `buf` value will get translated as necessary.
pub struct MemoryMessage {
    /// A user-assignable message ID.
    pub id: MessageId,

    /// The offset of the buffer.  This address will get transformed when the
    /// message is moved between processes.
    pub buf: MemoryRange,

    /// The offset within the buffer where the interesting stuff starts.
    pub offset: Option<MemoryAddress>,

    /// How many bytes in the buffer are valid
    pub valid: Option<MemorySize>,
}

impl MemoryMessage {
    pub fn from_usize(
        id: usize,
        addr: usize,
        size: usize,
        offset: usize,
        valid: usize,
    ) -> Option<MemoryMessage> {
        let addr = match MemoryAddress::new(addr) {
            None => return None,
            Some(s) => s,
        };
        let size = match MemorySize::new(size) {
            None => return None,
            Some(s) => s,
        };
        let buf = MemoryRange { addr, size };
        let offset = MemoryAddress::new(offset);
        let valid = MemorySize::new(valid);

        Some(MemoryMessage {
            id,
            buf,
            offset,
            valid,
        })
    }
    pub fn to_usize(&self) -> [usize; 5] {
        [
            self.id,
            self.buf.addr.get(),
            self.buf.size.get(),
            self.offset.map(|e| e.get()).unwrap_or(0),
            self.valid.map(|e| e.get()).unwrap_or(0),
        ]
    }
}

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy)]
/// A simple scalar message.  This is similar to a `move` message.
pub struct ScalarMessage {
    pub id: MessageId,
    pub arg1: usize,
    pub arg2: usize,
    pub arg3: usize,
    pub arg4: usize,
}

impl ScalarMessage {
    pub fn from_usize(
        id: usize,
        arg1: usize,
        arg2: usize,
        arg3: usize,
        arg4: usize,
    ) -> ScalarMessage {
        ScalarMessage {
            id,
            arg1,
            arg2,
            arg3,
            arg4,
        }
    }
    pub fn to_usize(&self) -> [usize; 5] {
        [self.id, self.arg1, self.arg2, self.arg3, self.arg4]
    }
}

#[repr(usize)]
#[derive(Debug, PartialEq)]
pub enum Message {
    MutableBorrow(MemoryMessage),
    Borrow(MemoryMessage),
    Move(MemoryMessage),
    Scalar(ScalarMessage),
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct MessageEnvelope {
    pub sender: MessageSender,
    pub message: Message,
}

impl MessageEnvelope {
    pub fn to_usize(&self) -> [usize; 7] {
        let ret = match &self.message {
            Message::MutableBorrow(m) => (0, m.to_usize()),
            Message::Borrow(m) => (1, m.to_usize()),
            Message::Move(m) => (2, m.to_usize()),
            Message::Scalar(m) => (3, m.to_usize()),
        };
        [
            self.sender,
            ret.0,
            ret.1[0],
            ret.1[1],
            ret.1[2],
            ret.1[3],
            ret.1[4],
        ]
    }
}

#[cfg(not(feature = "forget-memory-messages"))]
/// When a MessageEnvelope goes out of scope, return the memory.  It must either
/// go to the kernel (in the case of a Move), or back to the borrowed process
/// (in the case of a Borrow).  Ignore Scalar messages.
impl Drop for MessageEnvelope {
    fn drop(&mut self) {
        match &self.message {
            Message::ImmutableBorrow(x) | Message::MutableBorrow(x) => {
                crate::syscall::return_memory(self.sender, x.buf.addr.get(), x.buf.size.get())
                    .expect("couldn't return memory")
            }
            Message::Move(msg) => crate::syscall::unmap_memory(msg.buf.addr, msg.buf.size)
                .expect("couldn't free memory message"),
            _ => (),
        }
    }
}

impl MemoryRange {
    pub fn new(addr: usize, size: usize) -> MemoryRange {
        assert!(
            addr != 0,
            "tried to construct a memory range with a null pointer"
        );
        MemoryRange {
            addr: MemoryAddress::new(addr).unwrap(),
            size: MemorySize::new(size).unwrap(),
        }
    }

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
    Message(MessageEnvelope),
    ThreadID(TID),
    ProcessID(PID),

    /// The requested system call is unimplemented
    Unimplemented,

    /// The process is blocked and should perform the read() again
    BlockedProcess,

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
            Result::ServerID(sid) => [6, sid.0 as _, sid.1 as _, sid.2 as _, sid.3 as _, 0, 0, 0],
            Result::ConnectionID(cid) => [7, *cid, 0, 0, 0, 0, 0, 0],
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
            6 => Result::ServerID((src[1] as _, src[2] as _, src[3] as _, src[4] as _)),
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
                    _ => return Result::Error(Error::InternalError),
                };
                Result::Message(MessageEnvelope { sender, message })
            }
            9 => Result::ThreadID(src[1] as TID),
            10 => Result::ProcessID(PID::new(src[1] as _).unwrap()),
            11 => Result::Unimplemented,
            12 => Result::BlockedProcess,
            _ => Result::UnknownResult(src[1], src[2], src[3], src[4], src[5], src[6], src[7]),
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
