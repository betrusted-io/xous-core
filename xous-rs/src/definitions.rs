use core::num::NonZeroUsize;

pub type MemoryAddress = NonZeroUsize;
pub type MemorySize = NonZeroUsize;
pub type StackPointer = usize;
pub type MessageId = usize;

pub type PID = u8;
pub type MessageSender = usize;
pub type Connection = usize;

/// Server ID
pub type SID = usize;

/// Equivalent to a RISC-V Hart ID
pub type CpuID = usize;

#[repr(C)]
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
    UnhandledSyscall = 14,
}

#[repr(C)]
pub struct Context {
    stack: StackPointer,
    pid: PID,
}

#[repr(C)]
pub struct MemoryMessage {
    id: MessageId,
    in_buf: Option<MemoryAddress>,
    in_buf_size: Option<MemorySize>,
    out_buf: Option<MemoryAddress>,
    out_buf_size: Option<MemorySize>,
}

#[repr(C)]
pub struct ScalarMessage {
    id: MessageId,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
}

#[allow(dead_code)]
pub enum Message {
    Memory(MemoryMessage),
    Scalar(ScalarMessage),
}

#[allow(dead_code)]
pub struct MessageReceived {
    sender: MessageSender,
    message: Message,
}
