use crate::{
    CpuID, CtxID, Error, MemoryAddress, MemoryFlags, MemoryMessage, MemoryRange, MemorySize, Message,
    MessageEnvelope, MessageSender, ScalarMessage, CID, PID, SID,
};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

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

#[derive(Debug)]
pub enum SysCall {
    /// Allocates pages of memory, equal to a total of `size` bytes.  A physical
    /// address may be specified, which can be used to allocate regions such as
    /// memory-mapped I/O.
    ///
    /// If a virtual address is specified, then the returned pages are located
    /// at that address.  Otherwise, they are located at the Default offset.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't
    ///                     page-aligned, or the size isn't a multiple of the
    ///                     page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or
    ///                    the system's memory size has been exceeded.
    MapMemory(
        *mut usize,  /* phys */
        *mut usize,  /* virt */
        usize,       /* region size */
        MemoryFlags, /* flags */
    ),

    /// Release the memory back to the operating system.
    ///
    /// # Errors
    ///
    UnmapMemory(*mut usize /* virt */, usize /* region size */),

    /// Sets the offset and size of a given memory region.  This call may only
    /// be made by processes that have not yet started, or processes that have a
    /// PPID of 1. Care must be taken to ensure this region doesn't run into
    /// other regions. Additionally, the base address must avoid the kernel
    /// regions.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't
    ///                     page-aligned, or the size isn't a multiple of the
    ///                     page width.
    /// * **BadAddress**: The address conflicts with the kernel
    SetMemRegion(
        PID,        /* pid */
        MemoryType, /* region type */
        *mut usize, /* region address */
        usize,      /* region size */
    ),

    /// Add the given number of bytes to the heap.  The number of bytes must be
    /// divisible by the page size.  The newly-allocated pages will have the
    /// specified flags.  To get the current heap base, call this with a size of
    /// `0`.
    ///
    /// # Returns
    ///
    /// * **MemoryRange(*mut usize /* The base of the heap */, usize /* the new
    ///   size of the heap */)
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't
    ///                     page-aligned, or the size isn't a multiple of the
    ///                     page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or
    ///                    the system's memory size has been exceeded.
    IncreaseHeap(usize /* number of bytes to add */, MemoryFlags),

    /// Remove the given number of bytes from the heap.
    ///
    /// # Returns
    ///
    /// * **MemoryRange(*mut usize /* The base of the heap */, usize /* the new
    ///   size of the heap */)
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't
    ///                     page-aligned, or the size isn't a multiple of the
    ///                     page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or
    ///                    the system's memory size has been exceeded.
    DecreaseHeap(usize /* desired heap size */),

    /// Set the specified flags on the virtual address range. This can be used
    /// to REMOVE flags on a memory region, for example to mark it as no-execute
    /// after writing program data.
    ///
    /// # Errors
    ///
    /// * **ProcessNotChild**: The given PID is not a child of the current
    ///                        process.
    /// * **MemoryInUse**: The given PID has already been started, and it is not
    ///                    legal to modify memory flags anymore.
    UpdateMemoryFlags(
        *mut usize,  /* virt */
        usize,       /* number of pages */
        MemoryFlags, /* new flags */
    ),

    /// Pauses execution of the current thread and returns execution to the parent
    /// process.  This may return at any time in the future, including immediately.
    Yield,

    /// This process will now wait for an event such as an IRQ or Message.
    WaitEvent,

    /// This context will now wait for a message with the given server ID. You
    /// can set up a pool by having multiple threads call `ReceiveMessage` with
    /// the same SID.
    ReceiveMessage(SID),

    /// Stop running the given process and return control to the parent. This
    /// will force a Yield on the process currently running on the target CPU.
    /// This can be run during an Interrupt context.
    ///
    /// # Errors
    ///
    /// * **ProcessNotChild**: The given PID is not a child of the current
    ///   process
    ReturnToParentI(PID, CpuID),

    /// Claims an interrupt and unmasks it immediately.  The provided function
    /// will be called from within an interrupt context, but using the ordinary
    /// privilege level of the process.
    ///
    /// # Errors
    ///
    /// * **InterruptNotFound**: The specified interrupt isn't valid on this
    ///   system
    /// * **InterruptInUse**: The specified interrupt has already been claimed
    ClaimInterrupt(
        usize,      /* IRQ number */
        *mut usize, /* function pointer */
        *mut usize, /* argument */
    ),

    /// Returns the interrupt back to the operating system and masks it again.
    /// This function is implicitly called when a process exits.
    ///
    /// # Errors
    ///
    /// * **InterruptNotFound**: The specified interrupt doesn't exist, or isn't
    ///                          assigned to this process.
    FreeInterrupt(usize /* IRQ number */),

    /// Resumes a process using the given context.  A parent could use this
    /// function to implement multi-threading inside a child process, or to
    /// create a task switcher.
    ///
    /// To resume a process exactly where it left off, set `context_id` to `0`.
    /// This would be done in a very simple system that has no threads.
    ///
    /// If no more contexts are available when one is required, then the child
    /// automatically relinquishes its quantum.
    ///
    /// # Returns
    ///
    /// When this function returns, it provides a list of the processes and
    /// contexts that are ready to be run.  Three can fit as return values.
    ///
    /// # Examples
    ///
    /// If a process called `yield()`, or if its quantum expired normally, then
    /// a single pair is returned: (pid, context).
    ///
    /// If the child process called `client_send()` and ended up blocking due to
    /// the server not being ready, then this would return no pairs. This thread
    /// or process should not be scheduled to run.
    ///
    /// If the child called `client_send()` and the server was ready, then the
    /// server process would be run immediately.  If the child process' quantum
    /// expired while the server was running, then this function would return a
    /// single pair containing the PID of the server, and the context number.
    ///
    /// If the child called `client_send()` and the server was ready, then the
    /// server process would be run immediately.  If the server then finishes,
    /// execution flow is returned to the child process.  If the quantum then
    /// expires, this would return two pairs: the server's PID and its context
    /// when it called `client_reply()`, and the child's PID with its current
    /// context.
    ///
    /// If the server in turn called another server, and both servers ended up
    /// returning to the child before the quantum expired, then there would be
    /// three pairs returned.
    ///
    /// # Errors
    ///
    /// * **ProcessNotFound**: The requested process does not exist
    /// * **ProcessNotChild**: The given process was not a child process, and
    ///                        therefore couldn't be resumed.
    /// * **ProcessTerminated**: The process has crashed.
    SwitchTo(PID, usize /* context ID */),

    /// Get a list of contexts that can be run in the given PID.
    ReadyContexts(PID),

    /// Create a new Server
    ///
    /// This will return a 128-bit Server ID that can be used to send messages
    /// to this server.  This ID will be unique per process.  You may specify an
    /// additional `usize` value to make the ID unique.  This value will be
    /// mixed in with the random value.
    ///
    /// # Returns
    ///
    /// The ServerId can be assembled to form a 128-bit server ID in native byte
    /// order.
    ///
    /// # Errors
    ///
    /// * **OutOfMemory**: The server table was full and a new server couldn't
    ///                    be created.
    /// * **ServerExists**: The server hash is already in use.
    CreateServer(usize /* server hash */),

    /// Connect to a server.   This turns a 128-bit Serever ID into a 32-bit
    /// Connection ID.
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The server could not be found.
    Connect(SID /* server id */),

    /// Send a message to a server
    SendMessage(CID, Message),

    /// Return a Borrowed memory region to a sender
    ReturnMemory(MessageSender),

    /// Spawn a new thread
    SpawnThread(
        *mut usize, /* entrypoint */
        *mut usize, /* stack pointer */
        *mut usize, /* argument */
    ),

    /// This syscall does not exist
    Invalid(usize, usize, usize, usize, usize, usize, usize),
}

#[derive(FromPrimitive)]
enum SysCallNumber {
    MapMemory = 2,
    Yield = 3,
    ReturnToParentI = 4,
    ClaimInterrupt = 5,
    FreeInterrupt = 6,
    SwitchTo = 7,
    ReadyContexts = 8,
    WaitEvent = 9,
    IncreaseHeap = 10,
    DecreaseHeap = 11,
    UpdateMemoryFlags = 12,
    SetMemRegion = 13,
    CreateServer = 14,
    ReceiveMessage = 15,
    SendMessage = 16,
    Connect = 17,
    SpawnThread = 18,
    UnmapMemory = 19,
    ReturnMemory = 20,
    Invalid,
}

impl SysCall {
    pub fn as_args(&self) -> [usize; 8] {
        use core::mem;
        assert!(
            mem::size_of::<SysCall>() == mem::size_of::<usize>() * 8,
            "SysCall is not the expected size"
        );
        match self {
            SysCall::MapMemory(a1, a2, a3, a4) => [
                SysCallNumber::MapMemory as usize,
                *a1 as usize,
                *a2 as usize,
                *a3,
                a4.bits(),
                0,
                0,
                0,
            ],
            SysCall::UnmapMemory(a1, a2) => [
                SysCallNumber::UnmapMemory as usize,
                *a1 as usize,
                *a2,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::Yield => [SysCallNumber::Yield as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::WaitEvent => [SysCallNumber::WaitEvent as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::ReceiveMessage(sid) => [
                SysCallNumber::ReceiveMessage as usize,
                sid.0,
                sid.1,
                sid.2,
                sid.3,
                0,
                0,
                0,
            ],
            SysCall::ReturnToParentI(a1, a2) => [
                SysCallNumber::ReturnToParentI as usize,
                *a1 as usize,
                *a2 as usize,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::ClaimInterrupt(a1, a2, a3) => [
                SysCallNumber::ClaimInterrupt as usize,
                *a1,
                *a2 as usize,
                *a3 as usize,
                0,
                0,
                0,
                0,
            ],
            SysCall::FreeInterrupt(a1) => {
                [SysCallNumber::FreeInterrupt as usize, *a1, 0, 0, 0, 0, 0, 0]
            }
            SysCall::SwitchTo(a1, a2) => [
                SysCallNumber::SwitchTo as usize,
                *a1 as usize,
                *a2 as usize,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::ReadyContexts(a1) => [
                SysCallNumber::ReadyContexts as usize,
                *a1 as usize,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::IncreaseHeap(a1, a2) => [
                SysCallNumber::IncreaseHeap as usize,
                *a1 as usize,
                a2.bits(),
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::DecreaseHeap(a1) => [
                SysCallNumber::DecreaseHeap as usize,
                *a1 as usize,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::UpdateMemoryFlags(a1, a2, a3) => [
                SysCallNumber::UpdateMemoryFlags as usize,
                *a1 as usize,
                *a2 as usize,
                a3.bits(),
                0,
                0,
                0,
                0,
            ],
            SysCall::SetMemRegion(a1, a2, a3, a4) => [
                SysCallNumber::SetMemRegion as usize,
                *a1 as usize,
                *a2 as usize,
                *a3 as usize,
                *a4,
                0,
                0,
                0,
            ],

            SysCall::CreateServer(a1) => {
                [SysCallNumber::CreateServer as usize, *a1, 0, 0, 0, 0, 0, 0]
            }
            SysCall::Connect(sid) => [
                SysCallNumber::Connect as usize,
                sid.0,
                sid.1,
                sid.2,
                sid.3,
                0,
                0,
                0,
            ],
            SysCall::SendMessage(a1, ref a2) => match a2 {
                Message::MutableBorrow(mm) => [
                    SysCallNumber::SendMessage as usize,
                    *a1,
                    1,
                    mm.id as usize,
                    mm.buf.as_ptr() as usize,
                    mm.buf.len(),
                    mm.offset.map(|x| x.get()).unwrap_or(0) as usize,
                    mm.valid.map(|x| x.get()).unwrap_or(0) as usize,
                ],
                Message::ImmutableBorrow(mm) => [
                    SysCallNumber::SendMessage as usize,
                    *a1,
                    2,
                    mm.id as usize,
                    mm.buf.as_ptr() as usize,
                    mm.buf.len(),
                    mm.offset.map(|x| x.get()).unwrap_or(0) as usize,
                    mm.valid.map(|x| x.get()).unwrap_or(0) as usize,
                ],
                Message::Move(mm) => [
                    SysCallNumber::SendMessage as usize,
                    *a1,
                    3,
                    mm.id as usize,
                    mm.buf.as_ptr() as usize,
                    mm.buf.len(),
                    mm.offset.map(|x| x.get()).unwrap_or(0) as usize,
                    mm.valid.map(|x| x.get()).unwrap_or(0) as usize,
                ],
                Message::Scalar(sc) => [
                    SysCallNumber::SendMessage as usize,
                    *a1,
                    4,
                    sc.id as usize,
                    sc.arg1,
                    sc.arg2,
                    sc.arg3,
                    sc.arg4,
                ],
            },
            SysCall::ReturnMemory(a1) => [SysCallNumber::ReturnMemory as usize,
            *a1, 0, 0, 0, 0, 0, 0],
            SysCall::SpawnThread(a1, a2, a3) => [
                SysCallNumber::SpawnThread as usize,
                *a1 as usize,
                *a2 as usize,
                *a3 as usize,
                0,
                0,
                0,
                0,
            ],
            SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7) => [
                SysCallNumber::Invalid as usize,
                *a1,
                *a2,
                *a3,
                *a4,
                *a5,
                *a6,
                *a7,
            ],
        }
    }
    pub fn from_args(
        a0: usize,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
    ) -> core::result::Result<Self, Error> {
        Ok(match FromPrimitive::from_usize(a0) {
            Some(SysCallNumber::MapMemory) => SysCall::MapMemory(
                a1 as *mut usize,
                a2 as *mut usize,
                a3,
                MemoryFlags::from_bits(a4).ok_or(Error::InvalidSyscall)?,
            ),
            Some(SysCallNumber::UnmapMemory) => SysCall::UnmapMemory(a1 as *mut usize, a2),
            Some(SysCallNumber::Yield) => SysCall::Yield,
            Some(SysCallNumber::WaitEvent) => SysCall::WaitEvent,
            Some(SysCallNumber::ReceiveMessage) => SysCall::ReceiveMessage((a1, a2, a3, a4)),
            Some(SysCallNumber::ReturnToParentI) => SysCall::ReturnToParentI(a1 as PID, a2),
            Some(SysCallNumber::ClaimInterrupt) => {
                SysCall::ClaimInterrupt(a1, a2 as *mut usize, a3 as *mut usize)
            }
            Some(SysCallNumber::FreeInterrupt) => SysCall::FreeInterrupt(a1),
            Some(SysCallNumber::SwitchTo) => SysCall::SwitchTo(a1 as PID, a2 as usize),
            Some(SysCallNumber::ReadyContexts) => SysCall::ReadyContexts(a1 as u8),
            Some(SysCallNumber::IncreaseHeap) => SysCall::IncreaseHeap(
                a1 as usize,
                MemoryFlags::from_bits(a2).ok_or(Error::InvalidSyscall)?,
            ),
            Some(SysCallNumber::DecreaseHeap) => SysCall::DecreaseHeap(a1 as usize),
            Some(SysCallNumber::UpdateMemoryFlags) => SysCall::UpdateMemoryFlags(
                a1 as *mut usize,
                a2 as usize,
                MemoryFlags::from_bits(a3).ok_or(Error::InvalidSyscall)?,
            ),
            Some(SysCallNumber::SetMemRegion) => {
                SysCall::SetMemRegion(a1 as PID, MemoryType::from(a2), a3 as *mut usize, a4)
            }
            Some(SysCallNumber::CreateServer) => SysCall::CreateServer(a1),
            Some(SysCallNumber::Connect) => SysCall::Connect((a1, a2, a3, a4)),
            Some(SysCallNumber::SendMessage) => match a2 {
                1 => SysCall::SendMessage(
                    a1,
                    Message::MutableBorrow(MemoryMessage {
                        id: a3,
                        buf: MemoryRange::new(a4, a5),
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                2 => SysCall::SendMessage(
                    a1,
                    Message::ImmutableBorrow(MemoryMessage {
                        id: a3,
                        buf: MemoryRange::new(a4, a5),
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                3 => SysCall::SendMessage(
                    a1,
                    Message::Move(MemoryMessage {
                        id: a3,
                        buf: MemoryRange::new(a4, a5),
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                4 => SysCall::SendMessage(
                    a1,
                    Message::Scalar(ScalarMessage {
                        id: a3,
                        arg1: a4,
                        arg2: a5,
                        arg3: a6,
                        arg4: a7,
                    }),
                ),
                _ => SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7),
            },
            Some(SysCallNumber::ReturnMemory) => SysCall::ReturnMemory(a1),
            Some(SysCallNumber::SpawnThread) => {
                SysCall::SpawnThread(a1 as *mut usize, a2 as *mut usize, a3 as *mut usize)
            }
            Some(SysCallNumber::Invalid) => SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7),
            None => return Err(Error::InvalidSyscall),
        })
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
    UnknownResult(usize, usize, usize, usize, usize, usize, usize),
}

impl From<Error> for Result {
    fn from(e: Error) -> Self {
        Result::Error(e)
    }
}

pub type SyscallResult = core::result::Result<Result, Error>;

extern "Rust" {
    fn _xous_syscall_rust(
        nr: usize,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
        ret: &mut Result,
    );
    fn _xous_syscall(
        nr: usize,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
        ret: &mut Result,
    );
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn map_memory(
    phys: Option<MemoryAddress>,
    virt: Option<MemoryAddress>,
    size: usize,
    flags: MemoryFlags,
) -> core::result::Result<MemoryRange, Error> {
    let result = rsyscall(SysCall::MapMemory(
        phys.map(|x| x.get()).unwrap_or(0) as *mut usize,
        virt.map(|x| x.get()).unwrap_or(0) as *mut usize,
        size,
        flags,
    ))?;
    if let Result::MemoryRange(range) = result {
        Ok(range)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn unmap_memory(virt: MemoryAddress, size: MemorySize) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::UnmapMemory(virt.get() as *mut usize, size.get()))?;
    if let Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_memory(sender: MessageSender) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnMemory(sender))?;
    if let Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Claim a hardware interrupt for this process.
pub fn claim_interrupt(
    irq_no: usize,
    callback: fn(irq_no: usize, arg: *mut usize),
    arg: *mut usize,
) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ClaimInterrupt(
        irq_no,
        callback as *mut usize,
        arg as *mut usize,
    ))?;
    if let Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Create a new server with the given name.  This enables other processes to
/// connect to this server to send messages.  The name is a UTF-8 token that
/// will be mixed with other random data that is unique to each process.
/// That way, if a process crashes and is restarted, it can keep the same
/// name.  However, other processes cannot spoof this process.
///
/// # Errors
///
/// * **ServerExists**: A server has already registered with that name
/// * **InvalidString**: The name was not a valid UTF-8 string
pub fn create_server(name: usize) -> core::result::Result<SID, Error> {
    let result = rsyscall(SysCall::CreateServer(name))?;
    if let Result::ServerID(sid) = result {
        Ok(sid)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Connect to a server with the given SID
pub fn connect(server: SID) -> core::result::Result<CID, Error> {
    let result = rsyscall(SysCall::Connect(server))?;
    if let Result::ConnectionID(cid) = result {
        Ok(cid)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Suspend the current process until a message is received.  This thread will
/// block until a message is received.
///
/// # Errors
///
pub fn receive_message(server: SID) -> core::result::Result<MessageEnvelope, Error> {
    let result = rsyscall(SysCall::ReceiveMessage(server)).expect("Couldn't call watimessage");
    if let Result::Message(envelope) = result {
        Ok(envelope)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Send a message to a server.  Depending on the mesage type (move or borrow), it
/// will either block (borrow) or return immediately (move).
/// If the message type is `borrow`, then the memory addresses pointed to will be
/// unavailable to this process until this function returns.
///
/// # Errors
///
/// * **ServerNotFound**: The server does not exist so the connection is now invalid
/// * **BadAddress**: The client tried to pass a Memory message using an address it doesn't own
/// * **Timeout**: The timeout limit has been reached
pub fn send_message(connection: CID, message: Message) -> core::result::Result<(), Error> {
    let result =
        rsyscall(SysCall::SendMessage(connection, message)).expect("couldn't send message");
    if result == Result::Ok {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        panic!("Unexpected return value: {:?}", result);
    }
}

/// Return execution to the kernel. This function may return at any time,
/// including immediately
pub fn yield_slice() {
    rsyscall(SysCall::Yield).expect("yield_slice returned an error");
}

/// Return execution to the kernel and wait for a message or an interrupt.
pub fn wait_event() {
    rsyscall(SysCall::WaitEvent).expect("wait_event returned an error");
}

pub fn rsyscall(call: SysCall) -> SyscallResult {
    use core::mem::MaybeUninit;
    let mut ret = unsafe { MaybeUninit::uninit().assume_init() };
    let args = call.as_args();
    unsafe {
        _xous_syscall(
            args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], &mut ret,
        )
    };
    match ret {
        Result::Error(e) => Err(e),
        other => Ok(other),
    }
}

/// This is dangerous, but fast.
pub fn dangerous_syscall(call: SysCall) -> SyscallResult {
    use core::mem::{transmute, MaybeUninit};
    let mut ret = unsafe { MaybeUninit::uninit().assume_init() };
    let presto =
        unsafe { transmute::<_, (usize, usize, usize, usize, usize, usize, usize, usize)>(call) };
    unsafe {
        _xous_syscall_rust(
            presto.0, presto.1, presto.2, presto.3, presto.4, presto.5, presto.6, presto.7,
            &mut ret,
        )
    };
    match ret {
        Result::Error(e) => Err(e),
        other => Ok(other),
    }
}
