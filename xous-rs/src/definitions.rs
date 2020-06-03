use core::num::NonZeroUsize;

pub type MemoryAddress = NonZeroUsize;
pub type MemorySize = NonZeroUsize;
pub type StackPointer = usize;
pub type MessageId = usize;

pub type PID = u8;
pub type MessageSender = usize;
pub type Connection = usize;

/// Server ID
pub type SID = (usize, usize, usize, usize);

/// Connection ID
pub type CID = usize;

/// Context ID
pub type CtxID = usize;

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
    ContextNotAvailable = 16,
    UnhandledSyscall = 17,
    InvalidSyscall = 18,
    ShareViolation = 19,
    UnknownError = 20,
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
            16 => ContextNotAvailable,
            17 => UnhandledSyscall,
            18 => InvalidSyscall,
            19 => ShareViolation,
            _ => UnknownError,
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
}

#[repr(C)]
#[derive(Debug, PartialEq)]
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
}

#[repr(usize)]
#[derive(Debug, PartialEq)]
pub enum Message {
    MutableBorrow(MemoryMessage),
    ImmutableBorrow(MemoryMessage),
    Move(MemoryMessage),
    Scalar(ScalarMessage),
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub struct MessageEnvelope {
    pub sender: MessageSender,
    pub message: Message,
}

#[cfg(not(feature = "forget-memory-messages"))]
/// When a MessageEnvelope goes out of scope, return the memory.  It must either
/// go to the kernel (in the case of a Move), or back to the borrowed process
/// (in the case of a Borrow).  Ignore Scalar messages.
impl Drop for MessageEnvelope {
    fn drop(&mut self) {
        let (arg1, arg2) = match &self.message {
            Message::ImmutableBorrow(x) | Message::MutableBorrow(x) => (
                x.valid.map(|x| x.get()).unwrap_or(0),
                x.offset.map(|x| x.get()).unwrap_or(0),
            ),
            _ => (0, 0),
        };
        crate::syscall::return_memory(self.sender, arg1, arg2).expect("couldn't return memory");
        if let Message::Move(msg) = &self.message {
            crate::syscall::unmap_memory(msg.buf.addr, msg.buf.size)
                .expect("couldn't free memory message");
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

    pub fn len(&self) -> usize {
        self.size.get()
    }

    pub fn is_empty(&self) -> bool {
        self.size.get() > 0
    }

    pub fn as_ptr(&self) -> *const usize {
        self.addr.get() as *const usize
    }

    pub fn as_mut_ptr(&self) -> *mut usize {
        self.addr.get() as *mut usize
    }
}

/// Which memory region the operation should affect.
#[derive(Debug, Copy, Clone)]
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
    MemoryAddress(*mut u8),
    MemoryRange(MemoryRange),
    ReadyContexts(
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
    ThreadID(CtxID),
    MessageResult(usize, usize),
    UnknownResult(usize, usize, usize, usize, usize, usize, usize),
}

impl Result {
    pub fn from_args(src: [usize; 8]) -> Self {
        match src[0] {
            0 => Result::Ok,
            1 => Result::Error(Error::from_usize(src[1])),
            2 => Result::MemoryAddress(src[1] as *mut u8),
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
            4 => Result::ReadyContexts(src[1], src[2], src[3], src[4], src[5], src[6], src[7]),
            5 => Result::ResumeProcess,
            6 => Result::ServerID((src[1], src[2], src[3], src[4])),
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
                        Some(s) => Message::ImmutableBorrow(s),
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
            9 => Result::ThreadID(src[1] as CtxID),
            10 => Result::MessageResult(src[1], src[2]),
            _ => Result::UnknownResult(src[1], src[2], src[3], src[4], src[5], src[6], src[7]),
        }
    }
}

impl From<Error> for Result {
    fn from(e: Error) -> Self {
        Result::Error(e)
    }
}

pub type SyscallResult = core::result::Result<Result, Error>;
