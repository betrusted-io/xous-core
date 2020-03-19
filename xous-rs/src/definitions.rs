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

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

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
    UnhandledSyscall = 15,
}

#[repr(C)]
pub struct Context {
    stack: StackPointer,
    pid: PID,
}

#[repr(C)]
#[derive(Debug)]
/// A struct describing memory that is passed between processes.
/// The `buf` value will get translated as necessary.
pub struct MemoryMessage {

    /// A user-assignable message ID.
    pub id: MessageId,

    /// The offset of the buffer.  This address will get
    /// transformed when the message is moved between
    /// processes.
    pub buf: Option<MemoryAddress>,

    /// The overall size of the buffer.  Must be a multiple
    /// of PAGE_SIZE.
    pub buf_size: Option<MemorySize>,

    /// The offset within the buffer where the interesting
    /// stuff starts.
    pub _offset: Option<MemorySize>,

    /// How many bytes in the buffer are valid
    pub _valid: Option<MemorySize>,
}

#[repr(C)]
#[derive(Debug)]
/// A simple scalar message.  This is similar to a `move` message.
pub struct ScalarMessage {
    pub id: MessageId,
    pub arg1: usize,
    pub arg2: usize,
    pub arg3: usize,
    pub arg4: usize,
}

#[repr(usize)]
#[derive(Debug)]
pub enum Message {
    MutableBorrow(MemoryMessage),
    ImmutableBorrow(MemoryMessage),
    Move(MemoryMessage),
    Scalar(ScalarMessage),
}

#[repr(C)]
#[derive(Debug)]
pub struct MessageEnvelope {
    sender: MessageSender,
    message: Message,
}
