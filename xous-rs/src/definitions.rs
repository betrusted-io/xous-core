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
    pub offset: Option<MemorySize>,

    /// How many bytes in the buffer are valid
    pub valid: Option<MemorySize>,
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
        crate::syscall::return_memory(self.sender).expect("couldn't return memory");
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

    pub fn as_ptr(&self) -> *const usize {
        self.addr.get() as *const usize
    }

    pub fn as_mut_ptr(&self) -> *mut usize {
        self.addr.get() as *mut usize
    }
}
