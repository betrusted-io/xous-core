use core::convert::{TryFrom, TryInto};

/* https://github.com/betrusted-io/xous-core/issues/90
use crate::Exception
*/
// use num_derive::FromPrimitive;
// use num_traits::FromPrimitive;
#[cfg(feature = "processes-as-threads")]
pub use crate::arch::ProcessArgsAsThread;
use crate::{
    pid_from_usize, CpuID, Error, MemoryAddress, MemoryFlags, MemoryMessage, MemoryRange, MemorySize,
    MemoryType, Message, MessageEnvelope, MessageSender, ProcessArgs, ProcessInit, Result, ScalarMessage,
    SysCallResult, ThreadInit, CID, PID, SID, TID,
};

#[derive(Debug, PartialEq)]
pub enum SysCall {
    /// Allocates pages of memory, equal to a total of `size` bytes.  A physical
    /// address may be specified, which can be used to allocate regions such as
    /// memory-mapped I/O.
    ///
    /// If a virtual address is specified, then the returned pages are located
    /// at that address.  Otherwise, they are located at the Default offset.
    ///
    /// # Returns
    ///
    /// * **MemoryRange**: A memory range containing zeroed bytes.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned, or the size isn't a
    ///   multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's memory size has
    ///   been exceeded.
    MapMemory(
        Option<MemoryAddress>, /* phys */
        Option<MemoryAddress>, /* virt */
        MemorySize,            /* region size */
        MemoryFlags,           /* flags */
    ),

    /// Release the memory back to the operating system.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: The memory range was not page-aligned
    /// * **BadAddress**: A page in the range was not mapped
    UnmapMemory(MemoryRange),

    /// Sets the offset and size of a given memory region.  This call may only
    /// be made by processes that have not yet started, or processes that have a
    /// PPID of 1. Care must be taken to ensure this region doesn't run into
    /// other regions. Additionally, the base address must avoid the kernel
    /// regions.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned, or the size isn't a
    ///   multiple of the page width.
    /// * **BadAddress**: The address conflicts with the kernel
    SetMemRegion(
        PID,           /* pid */
        MemoryType,    /* region type */
        MemoryAddress, /* region address */
        usize,         /* region size */
    ),

    /// Add the given number of bytes to the heap.  The number of bytes must be
    /// divisible by the page size.  The newly-allocated pages will have the
    /// specified flags.  To get the current heap base, call this with a size of
    /// `0`, which will return a `MemoryRange` containing the heap base and the size.
    ///
    /// # Returns
    ///
    /// * **MemoryRange( *mut usize, /* The newly-allocated offset */ usize,      /* The amount of data added
    ///   */ )**
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned, or the size isn't a
    ///   multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's memory size has
    ///   been exceeded.
    IncreaseHeap(usize /* number of bytes to add */, MemoryFlags),

    /// Remove the given number of bytes from the heap.
    ///
    /// # Returns
    ///
    /// * **MemoryRange(*mut usize /* The base of the heap */, usize /* the new size of the heap */)
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned, or the size isn't a
    ///   multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's memory size has
    ///   been exceeded.
    DecreaseHeap(usize /* desired heap size */),

    /// Set the specified flags on the virtual address range. This can be used
    /// to REMOVE flags on a memory region, for example to mark it as no-execute
    /// after writing program data.
    ///
    /// If `PID` is `None`, then modifies this process. Note that it is not legal
    /// to modify the memory range of another process that has been started already.
    ///
    /// # Returns
    ///
    /// * **Ok**: The call completed successfully
    ///
    /// # Errors
    ///
    /// * **ProcessNotChild**: The given PID is not a child of the current process.
    /// * **MemoryInUse**: The given PID has already been started, and it is not legal to modify memory flags
    ///   anymore.
    UpdateMemoryFlags(
        MemoryRange, /* range of memory to update flags for */
        MemoryFlags, /* new flags */
        Option<PID>, /* if present, indicates the process to modify */
    ),

    /// Pauses execution of the current thread and returns execution to the parent
    /// process.  This may return at any time in the future, including immediately.
    ///
    /// # Returns
    ///
    /// * **Ok**: The call completed successfully
    ///
    /// # Errors
    ///
    /// This syscall will never return an error.
    Yield,

    /// This process will now wait for an event such as an IRQ or Message.
    ///
    /// # Returns
    ///
    /// * **Ok**: The call completed successfully
    ///
    /// # Errors
    ///
    /// This syscall will never error.
    WaitEvent,

    /// This thread will now wait for a message with the given server ID. You
    /// can set up a pool by having multiple threads call `ReceiveMessage` with
    /// the same SID.
    ///
    /// # Returns
    ///
    /// * **MessageEnvelope**: A valid message from the queue
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The given SID is not active or has terminated
    /// * **ProcessNotFound**: The parent process terminated when we were getting ready to block. This is an
    ///   internal error.
    /// * **BlockedProcess**: When running in Hosted mode, this indicates that this thread is blocking.
    ReceiveMessage(SID),

    /// If a message is available for the specified server, return that message
    /// and resume execution. If no message is available, return `Result::None`
    /// immediately without blocking.
    ///
    /// # Returns
    ///
    /// * **Message**: A valid message from the queue
    /// * **None**: Indicates that no message was in the queue
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The given SID is not active or has terminated
    /// * **ProcessNotFound**: The parent process terminated when we were getting ready to block. This is an
    ///   internal error.
    TryReceiveMessage(SID),

    /// Stop running the given process and return control to the parent. This
    /// will force a Yield on the process currently running on the target CPU.
    /// This can be run during an Interrupt context.
    ///
    /// # Errors
    ///
    /// * **ProcessNotChild**: The given PID is not a child of the current process
    ReturnToParent(PID, CpuID),

    /// Claims an interrupt and unmasks it immediately.  The provided function
    /// will be called from within an interrupt context, but using the ordinary
    /// privilege level of the process.
    ///
    /// # Returns
    ///
    /// * **Ok**: The interrupt has been mapped to this process
    ///
    /// # Errors
    ///
    /// * **InterruptNotFound**: The specified interrupt isn't valid on this system
    /// * **InterruptInUse**: The specified interrupt has already been claimed
    ClaimInterrupt(
        usize,                 /* IRQ number */
        MemoryAddress,         /* function pointer */
        Option<MemoryAddress>, /* argument */
    ),

    /// Returns the interrupt back to the operating system and masks it again.
    /// This function is implicitly called when a process exits.
    ///
    /// # Errors
    ///
    /// * **InterruptNotFound**: The specified interrupt doesn't exist, or isn't assigned to this process.
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
    /// * **ProcessNotChild**: The given process was not a child process, and therefore couldn't be resumed.
    /// * **ProcessTerminated**: The process has crashed.
    SwitchTo(PID, usize /* context ID */),

    /// Get a list of contexts that can be run in the given PID.
    ///
    /// # Errors
    ///
    /// * **UnhandledSyscall**: This syscall is currently unimplemented.
    ReadyThreads(PID),

    /// Create a new Server with a specified address
    ///
    /// This will return a 128-bit Server ID that can be used to send messages
    /// to this server, as well as a connection ID.  This connection ID will be
    /// unique per process, while the server ID is available globally.
    ///
    /// # Returns
    ///
    /// * **NewServerID(sid, cid)**: The specified SID, along with the connection ID for this process to talk
    ///   to the server.
    ///
    /// # Errors
    ///
    /// * **OutOfMemory**: The server table was full and a new server couldn't be created.
    /// * **ServerExists**: The server hash is already in use.
    CreateServerWithAddress(SID /* server hash */),

    /// Connect to a server.   This turns a 128-bit Server ID into a 32-bit
    /// Connection ID. Blocks until the server is available.
    ///
    /// # Returns
    ///
    /// * **ConnectionID(cid)**: The new connection ID for communicating with the server.
    ///
    /// # Errors
    ///
    /// None
    Connect(SID /* server id */),

    /// Try to connect to a server.   This turns a 128-bit Server ID into a 32-bit
    /// Connection ID.
    ///
    /// # Returns
    ///
    /// * **ConnectionID(cid)**: The new connection ID for communicating with the server.
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The server could not be found.
    TryConnect(SID /* server id */),

    /// Send a message to a server (blocking until it's ready)
    ///
    /// # Returns
    ///
    /// * **Ok**: The Scalar / Send message was successfully sent
    /// * **Scalar1**: The Server returned a `Scalar1` value
    /// * **Scalar2**: The Server returned a `Scalar2` value
    /// * **Scalar5**: The Server returned a `Scalar5` value
    /// * **BlockedProcess**: In Hosted mode, the target process is now blocked
    /// * **Message**: For Scalar messages, this includes the args as returned by the server. For
    ///   MemoryMessages, this will include the Opcode, Offset, and Valid fields.
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The server could not be found.
    /// * **ProcessNotFound**: Internal error -- the parent process couldn't be found when blocking
    SendMessage(CID, Message),

    /// Try to send a message to a server
    ///
    /// # Returns
    ///
    /// * **Ok**: The Scalar / Send message was successfully sent, or the Borrow has finished
    /// * **Scalar1**: The Server returned a `Scalar1` value
    /// * **Scalar2**: The Server returned a `Scalar2` value
    /// * **Scalar5**: The Server returned a `Scalar5` value
    /// * **BlockedProcess**: In Hosted mode, the target process is now blocked
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The server could not be found.
    /// * **ServerQueueFull**: The server's mailbox is full
    /// * **ProcessNotFound**: Internal error -- the parent process couldn't be found when blocking
    TrySendMessage(CID, Message),

    /// Return a Borrowed memory region to the sender
    ReturnMemory(
        MessageSender,      /* source of this message */
        MemoryRange,        /* address of range */
        Option<MemorySize>, /* offset */
        Option<MemorySize>, /* valid */
    ),

    /// Return a scalar to the sender
    ReturnScalar1(MessageSender, usize),

    /// Return two scalars to the sender
    ReturnScalar2(MessageSender, usize, usize),

    /// Spawn a new thread
    CreateThread(ThreadInit),

    /// Create a new process, setting the current process as the parent ID.
    /// Starts the process immediately and returns a `ProcessStartup` value.
    CreateProcess(ProcessInit),

    /// Terminate the current process, closing all server connections.
    TerminateProcess(u32),

    /// Shut down the entire system
    Shutdown,

    /// Create a new Server
    ///
    /// This will return a 128-bit Server ID that can be used to send messages
    /// to this server. The returned Server ID is random.
    ///
    /// # Returns
    ///
    /// The SID, along with a Connection ID that can be used to immediately
    /// communicate with this process.
    ///
    /// # Errors
    ///
    /// * **OutOfMemory**: The server table was full and a new server couldn't be created.
    CreateServer,

    /// Returns a 128-bit server ID, but does not create the server itself.
    /// basically an API to access the TRNG inside the kernel.
    CreateServerId,

    /// Establish a connection in the given process to the given server. This
    /// call can be used by a nameserver to make server connections without
    /// disclosing SIDs.
    ConnectForProcess(PID, SID),

    /// Get the current Thread ID
    GetThreadId,

    /// Get the current Process ID
    GetProcessId,

    /// Destroys the given Server ID. All clients that are waiting will be woken
    /// up and will receive a `ServerNotFound` response.
    DestroyServer(SID),

    /// Disconnects from a Server. This invalidates the CID, which may be reused
    /// in a future reconnection.
    Disconnect(CID),

    /// Waits for a thread to finish, and returns the return value of that thread.
    JoinThread(TID),

    /// A function to call when there is an exception such as a memory fault
    /// or illegal instruction.
    SetExceptionHandler(usize /* function pointer */, usize /* stack pointer */),

    /// Adjust one of the limits within this process. Note that you must pass
    /// the current limit value in order to set the new limit. The current limit
    /// value is always returned, so this function may need to be called twice --
    /// once in order to get the current limit, and again to set the new limit.
    ///
    /// ## Arguments
    ///
    /// * **Index**: The item to adjust. Currently the following limits are supported: 1: Maximum heap size
    ///   2: Current heap size
    /// * **Current Limit**: Pass the current limit value here. The current limit must match in order for the
    ///   new limit to take effect. This is used to avoid a race condition if two threads try to set the same
    ///   limit.
    /// * **Proposed Limit**: The new value that you would like to use.
    ///
    /// ## Returns
    ///
    /// Returns a Scalar2 containing `(Index, Limit)`.
    ///
    /// ## Errors
    ///
    /// * **InvalidLimit**: The specified index was not valid
    AdjustProcessLimit(
        usize, /* process limit index */
        usize, /* expected current limit */
        usize, /* proposed new limit */
    ),

    /// Returns the physical address corresponding to a virtual address, if such a mapping exists.
    ///
    /// ## Arguments
    ///     * **vaddr**: The virtual address to inspect
    ///
    /// ## Returns
    /// Returns a Scalar1 containing the physical address
    ///
    /// ## Errors
    ///     * **BadAddress**: The mapping does not exist
    #[cfg(feature = "v2p")]
    VirtToPhys(usize /* virtual address */),

    /// Return five scalars to the sender
    ReturnScalar5(MessageSender, usize, usize, usize, usize, usize),

    /// Return a message with the given Message ID and the provided parameters,
    /// and listen for another message on the server ID that sent this message.
    /// This call is meant to be used in the main loop of a server in order to
    /// reduce latency when that server is called frequently.
    ///
    /// This call works on both `MemoryMessages` and `BlockingScalars`, and does
    /// not distinguish between the two from the API.
    ///
    /// ## Arguments
    ///
    /// * **MessageSender**: This is the `sender` from the message envelope. It is a unique ID that
    ///   identifies this message, as well as the server it came from.
    ///
    /// The remaining arguments depend on whether the message was a `BlockingScalar`
    /// message or a `MemoryMessage`. Note that this function should NOT be called
    /// on non-blocking messages such as `Send` or `Scalar`.
    ///
    /// ## Returns
    ///
    /// * **Message**: A valid message from the queue
    ///
    /// # Errors
    ///
    /// * **ServerNotFound**: The given SID is not active or has terminated
    /// * **ProcessNotFound**: The parent process terminated when we were getting ready to block. This is an
    ///   internal error.
    /// * **BlockedProcess**: When running in Hosted mode, this indicates that this thread is blocking.
    ReplyAndReceiveNext(
        MessageSender, /* ID if the sender that sent this message */
        usize,         /* Return code to the caller */
        usize,         /* arg1 (BlockingScalar) or the memory address (MemoryMesage) */
        usize,         /* arg2 (BlockingScalar) or the memory length (MemoryMesage) */
        usize,         /* arg3 (BlockingScalar) or the memory offset (MemoryMesage) */
        usize,         /* arg4 (BlockingScalar) or the memory valid (MemoryMesage) */
        usize,         /* how many args are valid (BlockingScalar) or usize::MAX (MemoryMessge) */
    ),

    /// Returns the physical address corresponding to a virtual address for a given process, if such a
    /// mapping exists.
    ///
    /// ## Arguments
    ///     * **pid**: The PID
    ///     * **vaddr**: The virtual address to inspect
    ///
    /// ## Returns
    /// Returns a Scalar1 containing the physical address
    ///
    /// ## Errors
    ///     * **BadAddress**: The mapping does not exist
    #[cfg(feature = "v2p")]
    VirtToPhysPid(PID /* Process ID */, usize /* virtual address */),

    /// This syscall does not exist. It captures all possible
    /// arguments so detailed analysis can be performed.
    Invalid(usize, usize, usize, usize, usize, usize, usize),
}

// #[derive(FromPrimitive)]
pub enum SysCallNumber {
    MapMemory = 2,
    Yield = 3,
    ReturnToParent = 4,
    ClaimInterrupt = 5,
    FreeInterrupt = 6,
    SwitchTo = 7,
    ReadyThreads = 8,
    WaitEvent = 9,
    IncreaseHeap = 10,
    DecreaseHeap = 11,
    UpdateMemoryFlags = 12,
    SetMemRegion = 13,
    CreateServerWithAddress = 14,
    ReceiveMessage = 15,
    SendMessage = 16,
    Connect = 17,
    CreateThread = 18,
    UnmapMemory = 19,
    ReturnMemory = 20,
    CreateProcess = 21,
    TerminateProcess = 22,
    Shutdown = 23,
    TrySendMessage = 24,
    TryConnect = 25,
    ReturnScalar1 = 26,
    ReturnScalar2 = 27,
    TryReceiveMessage = 28,
    CreateServer = 29,
    ConnectForProcess = 30,
    CreateServerId = 31,
    GetThreadId = 32,
    GetProcessId = 33,
    DestroyServer = 34,
    Disconnect = 35,
    JoinThread = 36,
    SetExceptionHandler = 37,
    AdjustProcessLimit = 38,
    #[cfg(feature = "v2p")]
    VirtToPhys = 39,
    ReturnScalar5 = 40,
    ReplyAndReceiveNext = 41,
    #[cfg(feature = "v2p")]
    VirtToPhysPid = 42,
    Invalid,
}

impl SysCallNumber {
    pub fn from(val: usize) -> SysCallNumber {
        use SysCallNumber::*;
        match val {
            2 => MapMemory,
            3 => Yield,
            4 => ReturnToParent,
            5 => ClaimInterrupt,
            6 => FreeInterrupt,
            7 => SwitchTo,
            8 => ReadyThreads,
            9 => WaitEvent,
            10 => IncreaseHeap,
            11 => DecreaseHeap,
            12 => UpdateMemoryFlags,
            13 => SetMemRegion,
            14 => CreateServerWithAddress,
            15 => ReceiveMessage,
            16 => SendMessage,
            17 => Connect,
            18 => CreateThread,
            19 => UnmapMemory,
            20 => ReturnMemory,
            21 => CreateProcess,
            22 => TerminateProcess,
            23 => Shutdown,
            24 => TrySendMessage,
            25 => TryConnect,
            26 => ReturnScalar1,
            27 => ReturnScalar2,
            28 => TryReceiveMessage,
            29 => CreateServer,
            30 => ConnectForProcess,
            31 => CreateServerId,
            32 => GetThreadId,
            33 => GetProcessId,
            34 => DestroyServer,
            35 => Disconnect,
            36 => JoinThread,
            37 => SetExceptionHandler,
            38 => AdjustProcessLimit,
            #[cfg(feature = "v2p")]
            39 => VirtToPhys,
            40 => ReturnScalar5,
            41 => ReplyAndReceiveNext,
            #[cfg(feature = "v2p")]
            42 => VirtToPhysPid,
            _ => Invalid,
        }
    }
}

impl SysCall {
    fn add_opcode(opcode: SysCallNumber, args: [usize; 7]) -> [usize; 8] {
        [opcode as usize, args[0], args[1], args[2], args[3], args[4], args[5], args[6]]
    }

    /// Convert the SysCall into an array of eight `usize` elements,
    /// suitable for passing to the kernel.
    pub fn as_args(&self) -> [usize; 8] {
        use core::mem;
        assert!(
            mem::size_of::<SysCall>() == mem::size_of::<usize>() * 8,
            "SysCall is not the expected size (expected {}, got {})",
            mem::size_of::<usize>() * 8,
            mem::size_of::<SysCall>()
        );
        match self {
            SysCall::MapMemory(a1, a2, a3, a4) => [
                SysCallNumber::MapMemory as usize,
                a1.map(|x| x.get()).unwrap_or_default(),
                a2.map(|x| x.get()).unwrap_or_default(),
                a3.get(),
                a4.bits(),
                0,
                0,
                0,
            ],
            SysCall::UnmapMemory(range) => {
                [SysCallNumber::UnmapMemory as usize, range.as_ptr() as usize, range.len(), 0, 0, 0, 0, 0]
            }
            SysCall::Yield => [SysCallNumber::Yield as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::WaitEvent => [SysCallNumber::WaitEvent as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::ReceiveMessage(sid) => {
                let s = sid.to_u32();
                [SysCallNumber::ReceiveMessage as usize, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            SysCall::TryReceiveMessage(sid) => {
                let s = sid.to_u32();
                [SysCallNumber::TryReceiveMessage as usize, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            SysCall::ConnectForProcess(pid, sid) => {
                let s = sid.to_u32();
                [
                    SysCallNumber::ConnectForProcess as usize,
                    pid.get() as _,
                    s.0 as _,
                    s.1 as _,
                    s.2 as _,
                    s.3 as _,
                    0,
                    0,
                ]
            }
            SysCall::CreateServerId => [SysCallNumber::CreateServerId as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::ReturnToParent(a1, a2) => {
                [SysCallNumber::ReturnToParent as usize, a1.get() as usize, *a2, 0, 0, 0, 0, 0]
            }
            SysCall::ClaimInterrupt(a1, a2, a3) => [
                SysCallNumber::ClaimInterrupt as usize,
                *a1,
                a2.get(),
                a3.map(|x| x.get()).unwrap_or_default(),
                0,
                0,
                0,
                0,
            ],
            SysCall::FreeInterrupt(a1) => [SysCallNumber::FreeInterrupt as usize, *a1, 0, 0, 0, 0, 0, 0],
            SysCall::SwitchTo(a1, a2) => {
                [SysCallNumber::SwitchTo as usize, a1.get() as usize, *a2, 0, 0, 0, 0, 0]
            }
            SysCall::ReadyThreads(a1) => {
                [SysCallNumber::ReadyThreads as usize, a1.get() as usize, 0, 0, 0, 0, 0, 0]
            }
            SysCall::IncreaseHeap(a1, a2) => {
                [SysCallNumber::IncreaseHeap as usize, *a1, a2.bits(), 0, 0, 0, 0, 0]
            }
            SysCall::DecreaseHeap(a1) => [SysCallNumber::DecreaseHeap as usize, *a1, 0, 0, 0, 0, 0, 0],
            SysCall::UpdateMemoryFlags(a1, a2, a3) => [
                SysCallNumber::UpdateMemoryFlags as usize,
                a1.as_mut_ptr() as usize,
                a1.len(),
                a2.bits(),
                a3.map(|m| m.get() as usize).unwrap_or(0),
                0,
                0,
                0,
            ],
            SysCall::SetMemRegion(a1, a2, a3, a4) => [
                SysCallNumber::SetMemRegion as usize,
                a1.get() as usize,
                *a2 as usize,
                a3.get(),
                *a4,
                0,
                0,
                0,
            ],

            SysCall::CreateServerWithAddress(sid) => {
                let s = sid.to_u32();
                [
                    SysCallNumber::CreateServerWithAddress as usize,
                    s.0 as _,
                    s.1 as _,
                    s.2 as _,
                    s.3 as _,
                    0,
                    0,
                    0,
                ]
            }
            SysCall::CreateServer => [SysCallNumber::CreateServer as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::Connect(sid) => {
                let s = sid.to_u32();
                [SysCallNumber::Connect as usize, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            SysCall::SendMessage(a1, ref a2) => match a2 {
                Message::MutableBorrow(mm) | Message::Borrow(mm) | Message::Move(mm) => [
                    SysCallNumber::SendMessage as usize,
                    *a1 as usize,
                    a2.message_type(),
                    mm.id,
                    mm.buf.as_ptr() as usize,
                    mm.buf.len(),
                    mm.offset.map(|x| x.get()).unwrap_or(0),
                    mm.valid.map(|x| x.get()).unwrap_or(0),
                ],
                Message::Scalar(sc) | Message::BlockingScalar(sc) => [
                    SysCallNumber::SendMessage as usize,
                    *a1 as usize,
                    a2.message_type(),
                    sc.id,
                    sc.arg1,
                    sc.arg2,
                    sc.arg3,
                    sc.arg4,
                ],
            },
            SysCall::ReturnMemory(sender, buf, offset, valid) => [
                SysCallNumber::ReturnMemory as usize,
                sender.to_usize(),
                buf.as_ptr() as usize,
                buf.len(),
                offset.map(|o| o.get()).unwrap_or_default(),
                valid.map(|v| v.get()).unwrap_or_default(),
                0,
                0,
            ],
            SysCall::ReplyAndReceiveNext(sender, arg0, arg1, arg2, arg3, arg4, return_type) => [
                SysCallNumber::ReplyAndReceiveNext as usize,
                sender.to_usize(),
                *arg0,
                *arg1,
                *arg2,
                *arg3,
                *arg4,
                *return_type,
            ],

            SysCall::CreateThread(init) => {
                crate::arch::thread_to_args(SysCallNumber::CreateThread as usize, init)
            }
            SysCall::CreateProcess(init) => Self::add_opcode(SysCallNumber::CreateProcess, init.into()),
            SysCall::TerminateProcess(exit_code) => {
                [SysCallNumber::TerminateProcess as usize, *exit_code as usize, 0, 0, 0, 0, 0, 0]
            }
            SysCall::Shutdown => [SysCallNumber::Shutdown as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::TryConnect(sid) => {
                let s = sid.to_u32();
                [SysCallNumber::TryConnect as usize, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            SysCall::TrySendMessage(a1, ref a2) => match a2 {
                Message::MutableBorrow(mm) | Message::Borrow(mm) | Message::Move(mm) => [
                    SysCallNumber::TrySendMessage as usize,
                    *a1 as usize,
                    a2.message_type(),
                    mm.id,
                    mm.buf.as_ptr() as usize,
                    mm.buf.len(),
                    mm.offset.map(|x| x.get()).unwrap_or(0),
                    mm.valid.map(|x| x.get()).unwrap_or(0),
                ],
                Message::Scalar(sc) | Message::BlockingScalar(sc) => [
                    SysCallNumber::TrySendMessage as usize,
                    *a1 as usize,
                    a2.message_type(),
                    sc.id,
                    sc.arg1,
                    sc.arg2,
                    sc.arg3,
                    sc.arg4,
                ],
            },
            SysCall::ReturnScalar1(sender, arg1) => {
                [SysCallNumber::ReturnScalar1 as usize, sender.to_usize(), *arg1, 0, 0, 0, 0, 0]
            }
            SysCall::ReturnScalar2(sender, arg1, arg2) => {
                [SysCallNumber::ReturnScalar2 as usize, sender.to_usize(), *arg1, *arg2, 0, 0, 0, 0]
            }
            SysCall::GetThreadId => [SysCallNumber::GetThreadId as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::GetProcessId => [SysCallNumber::GetProcessId as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::DestroyServer(sid) => {
                let s = sid.to_u32();
                [SysCallNumber::DestroyServer as usize, s.0 as _, s.1 as _, s.2 as _, s.3 as _, 0, 0, 0]
            }
            SysCall::Disconnect(cid) => [SysCallNumber::Disconnect as usize, *cid as usize, 0, 0, 0, 0, 0, 0],
            SysCall::JoinThread(tid) => [SysCallNumber::JoinThread as usize, *tid, 0, 0, 0, 0, 0, 0],
            SysCall::SetExceptionHandler(pc, sp) => {
                [SysCallNumber::SetExceptionHandler as usize, *pc, *sp, 0, 0, 0, 0, 0]
            }
            SysCall::AdjustProcessLimit(index, current, new) => {
                [SysCallNumber::AdjustProcessLimit as usize, *index, *current, *new, 0, 0, 0, 0]
            }
            #[cfg(feature = "v2p")]
            SysCall::VirtToPhys(vaddr) => [SysCallNumber::VirtToPhys as usize, *vaddr, 0, 0, 0, 0, 0, 0],
            SysCall::ReturnScalar5(sender, arg1, arg2, arg3, arg4, arg5) => [
                SysCallNumber::ReturnScalar5 as usize,
                sender.to_usize(),
                *arg1,
                *arg2,
                *arg3,
                *arg4,
                *arg5,
                0,
            ],
            #[cfg(feature = "v2p")]
            SysCall::VirtToPhysPid(pid, vaddr) => {
                [SysCallNumber::VirtToPhysPid as usize, pid.get() as usize, *vaddr, 0, 0, 0, 0, 0]
            }
            SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7) => {
                [SysCallNumber::Invalid as usize, *a1, *a2, *a3, *a4, *a5, *a6, *a7]
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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
        Ok(match SysCallNumber::from(a0) {
            SysCallNumber::MapMemory => SysCall::MapMemory(
                MemoryAddress::new(a1),
                MemoryAddress::new(a2),
                MemoryAddress::new(a3).ok_or(Error::InvalidSyscall)?,
                crate::MemoryFlags::from_bits(a4).ok_or(Error::InvalidSyscall)?,
            ),
            SysCallNumber::UnmapMemory => {
                SysCall::UnmapMemory(unsafe { MemoryRange::new(a1, a2).or(Err(Error::InvalidSyscall)) }?)
            }
            SysCallNumber::Yield => SysCall::Yield,
            SysCallNumber::WaitEvent => SysCall::WaitEvent,
            SysCallNumber::ReceiveMessage => {
                SysCall::ReceiveMessage(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _))
            }
            SysCallNumber::TryReceiveMessage => {
                SysCall::TryReceiveMessage(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _))
            }
            SysCallNumber::ReturnToParent => SysCall::ReturnToParent(pid_from_usize(a1)?, a2),
            SysCallNumber::ClaimInterrupt => SysCall::ClaimInterrupt(
                a1,
                MemoryAddress::new(a2).ok_or(Error::InvalidSyscall)?,
                MemoryAddress::new(a3),
            ),
            SysCallNumber::FreeInterrupt => SysCall::FreeInterrupt(a1),
            SysCallNumber::SwitchTo => SysCall::SwitchTo(pid_from_usize(a1)?, a2),
            SysCallNumber::ReadyThreads => SysCall::ReadyThreads(pid_from_usize(a1)?),
            SysCallNumber::IncreaseHeap => {
                SysCall::IncreaseHeap(a1, crate::MemoryFlags::from_bits(a2).ok_or(Error::InvalidSyscall)?)
            }
            SysCallNumber::DecreaseHeap => SysCall::DecreaseHeap(a1),
            SysCallNumber::UpdateMemoryFlags => SysCall::UpdateMemoryFlags(
                unsafe { MemoryRange::new(a1, a2) }?,
                crate::MemoryFlags::from_bits(a3).ok_or(Error::InvalidSyscall)?,
                PID::new(a4 as _),
            ),
            SysCallNumber::SetMemRegion => SysCall::SetMemRegion(
                pid_from_usize(a1)?,
                MemoryType::from(a2),
                MemoryAddress::new(a3).ok_or(Error::InvalidSyscall)?,
                a4,
            ),
            SysCallNumber::CreateServerWithAddress => {
                SysCall::CreateServerWithAddress(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _))
            }
            SysCallNumber::CreateServer => SysCall::CreateServer,
            SysCallNumber::Connect => SysCall::Connect(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _)),
            SysCallNumber::SendMessage => Message::try_from((a2, a3, a4, a5, a6, a7))
                .map(|m| SysCall::SendMessage(a1.try_into().unwrap(), m))
                .unwrap_or_else(|_| SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7)),
            SysCallNumber::ReturnMemory => SysCall::ReturnMemory(
                MessageSender::from_usize(a1),
                unsafe { MemoryRange::new(a2, a3) }?,
                MemorySize::new(a4),
                MemorySize::new(a5),
            ),
            SysCallNumber::ReplyAndReceiveNext => {
                SysCall::ReplyAndReceiveNext(MessageSender::from_usize(a1), a2, a3, a4, a5, a6, a7)
            }
            SysCallNumber::CreateThread => {
                SysCall::CreateThread(crate::arch::args_to_thread(a1, a2, a3, a4, a5, a6, a7)?)
            }
            SysCallNumber::CreateProcess => SysCall::CreateProcess([a1, a2, a3, a4, a5, a6, a7].try_into()?),
            SysCallNumber::TerminateProcess => SysCall::TerminateProcess(a1 as u32),
            SysCallNumber::Shutdown => SysCall::Shutdown,
            SysCallNumber::TryConnect => {
                SysCall::TryConnect(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _))
            }
            SysCallNumber::TrySendMessage => match a2 {
                1 => SysCall::TrySendMessage(
                    a1 as u32,
                    Message::MutableBorrow(MemoryMessage {
                        id: a3,
                        buf: unsafe { MemoryRange::new(a4, a5) }?,
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                2 => SysCall::TrySendMessage(
                    a1 as u32,
                    Message::Borrow(MemoryMessage {
                        id: a3,
                        buf: unsafe { MemoryRange::new(a4, a5) }?,
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                3 => SysCall::TrySendMessage(
                    a1 as u32,
                    Message::Move(MemoryMessage {
                        id: a3,
                        buf: unsafe { MemoryRange::new(a4, a5) }?,
                        offset: MemoryAddress::new(a6),
                        valid: MemorySize::new(a7),
                    }),
                ),
                4 => SysCall::TrySendMessage(
                    a1 as u32,
                    Message::Scalar(ScalarMessage { id: a3, arg1: a4, arg2: a5, arg3: a6, arg4: a7 }),
                ),
                5 => SysCall::TrySendMessage(
                    a1.try_into().unwrap(),
                    Message::BlockingScalar(ScalarMessage { id: a3, arg1: a4, arg2: a5, arg3: a6, arg4: a7 }),
                ),
                _ => SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7),
            },
            SysCallNumber::ReturnScalar1 => SysCall::ReturnScalar1(MessageSender::from_usize(a1), a2),
            SysCallNumber::ReturnScalar2 => SysCall::ReturnScalar2(MessageSender::from_usize(a1), a2, a3),
            SysCallNumber::ConnectForProcess => SysCall::ConnectForProcess(
                PID::new(a1 as _).ok_or(Error::InvalidSyscall)?,
                SID::from_u32(a2 as _, a3 as _, a4 as _, a5 as _),
            ),
            SysCallNumber::CreateServerId => SysCall::CreateServerId,
            SysCallNumber::GetThreadId => SysCall::GetThreadId,
            SysCallNumber::GetProcessId => SysCall::GetProcessId,
            SysCallNumber::DestroyServer => {
                SysCall::DestroyServer(SID::from_u32(a1 as _, a2 as _, a3 as _, a4 as _))
            }
            SysCallNumber::Disconnect => SysCall::Disconnect(a1 as _),
            SysCallNumber::JoinThread => SysCall::JoinThread(a1 as _),
            SysCallNumber::SetExceptionHandler => SysCall::SetExceptionHandler(a1 as _, a2 as _),
            SysCallNumber::AdjustProcessLimit => SysCall::AdjustProcessLimit(a1, a2, a3),
            #[cfg(feature = "v2p")]
            SysCallNumber::VirtToPhys => SysCall::VirtToPhys(a1 as _),
            #[cfg(feature = "v2p")]
            SysCallNumber::VirtToPhysPid => SysCall::VirtToPhysPid(pid_from_usize(a1)?, a2 as _),
            SysCallNumber::ReturnScalar5 => {
                SysCall::ReturnScalar5(MessageSender::from_usize(a1), a2, a3, a4, a5, a6)
            }
            SysCallNumber::Invalid => SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7),
        })
    }

    /// Returns `true` if the associated syscall is a message that has memory attached to it
    pub fn has_memory(&self) -> bool {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => {
                matches!(msg, Message::Move(_) | Message::Borrow(_) | Message::MutableBorrow(_))
            }
            SysCall::ReturnMemory(_, _, _, _) => true,
            SysCall::ReplyAndReceiveNext(_, _, _, _, _, _, usize::MAX) => true,
            _ => false,
        }
    }

    /// Returns `true` if the associated syscall is a message that is a Move
    pub fn is_move(&self) -> bool {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => {
                matches!(msg, Message::Move(_))
            }
            _ => false,
        }
    }

    /// Returns `true` if the associated syscall is a message that is a Borrow
    pub fn is_borrow(&self) -> bool {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => {
                matches!(msg, Message::Borrow(_))
            }
            _ => false,
        }
    }

    /// Returns `true` if the associated syscall is a message that is a MutableBorrow
    pub fn is_mutableborrow(&self) -> bool {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => {
                matches!(msg, Message::MutableBorrow(_))
            }
            _ => false,
        }
    }

    /// Returns `true` if the associated syscall is returning memory
    pub fn is_return_memory(&self) -> bool {
        matches!(self, SysCall::ReturnMemory(..) | SysCall::ReplyAndReceiveNext(_, _, _, _, _, _, usize::MAX))
    }

    /// If the syscall has memory attached to it, return the memory
    pub fn memory(&self) -> Option<MemoryRange> {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => match msg {
                Message::Move(memory_message)
                | Message::Borrow(memory_message)
                | Message::MutableBorrow(memory_message) => Some(memory_message.buf),
                _ => None,
            },
            SysCall::ReturnMemory(_, range, _, _) => Some(*range),
            SysCall::ReplyAndReceiveNext(_, _, a1, a2, _, _, usize::MAX) => unsafe {
                MemoryRange::new(*a1, *a2).ok()
            },
            _ => None,
        }
    }

    /// If the syscall has memory attached to it, replace the memory.
    ///
    /// # Safety
    ///
    /// This function is only safe to call to fixup the pointer, particularly
    /// when running in hosted mode. It should not be used for any other purpose.
    pub unsafe fn replace_memory(&mut self, new: MemoryRange) {
        match self {
            SysCall::TrySendMessage(_, msg) | SysCall::SendMessage(_, msg) => match msg {
                Message::Move(memory_message)
                | Message::Borrow(memory_message)
                | Message::MutableBorrow(memory_message) => memory_message.buf = new,
                _ => (),
            },
            SysCall::ReturnMemory(_, range, _, _) => *range = new,
            SysCall::ReplyAndReceiveNext(_, _, a1, a2, _, _, usize::MAX) => {
                *a1 = new.addr.get();
                *a2 = new.size.get();
            }
            _ => (),
        }
    }

    /// Returns `true` if the given syscall may be called from an IRQ context
    pub fn can_call_from_interrupt(&self) -> bool {
        if let SysCall::TrySendMessage(_cid, msg) = self {
            return !msg.is_blocking();
        }
        matches!(
            self,
            SysCall::TryConnect(_)
                | SysCall::FreeInterrupt(_)
                | SysCall::ClaimInterrupt(_, _, _)
                | SysCall::TryReceiveMessage(_)
                | SysCall::ReturnToParent(_, _)
                | SysCall::ReturnScalar5(_, _, _, _, _, _)
                | SysCall::ReturnScalar2(_, _, _)
                | SysCall::ReturnScalar1(_, _)
                | SysCall::ReturnMemory(_, _, _, _)
        )
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn map_memory(
    phys: Option<MemoryAddress>,
    virt: Option<MemoryAddress>,
    size: usize,
    flags: MemoryFlags,
) -> core::result::Result<MemoryRange, Error> {
    crate::arch::map_memory_pre(&phys, &virt, size, flags)?;
    let result =
        rsyscall(SysCall::MapMemory(phys, virt, MemorySize::new(size).ok_or(Error::InvalidSyscall)?, flags))?;
    if let Result::MemoryRange(range) = result {
        Ok(crate::arch::map_memory_post(phys, virt, size, flags, range)?)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn unmap_memory(range: MemoryRange) -> core::result::Result<(), Error> {
    crate::arch::unmap_memory_pre(&range)?;
    let result = rsyscall(SysCall::UnmapMemory(range))?;
    if let crate::Result::Ok = result {
        crate::arch::unmap_memory_post(range)?;
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Update the permissions on the given memory range. Note that permissions may
/// only be stripped here -- they may never be added.
pub fn update_memory_flags(range: MemoryRange, flags: MemoryFlags) -> core::result::Result<Result, Error> {
    let result = rsyscall(SysCall::UpdateMemoryFlags(range, flags, None))?;
    if let Result::Ok = result {
        Ok(Result::Ok)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_memory(sender: MessageSender, mem: MemoryRange) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnMemory(sender, mem, None, None))?;
    if let crate::Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_memory_offset(
    sender: MessageSender,
    mem: MemoryRange,
    offset: Option<MemorySize>,
) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnMemory(sender, mem, offset, None))?;
    if let crate::Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_memory_offset_valid(
    sender: MessageSender,
    mem: MemoryRange,
    offset: Option<MemorySize>,
    valid: Option<MemorySize>,
) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnMemory(sender, mem, offset, valid))?;
    if let crate::Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_scalar(sender: MessageSender, val: usize) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnScalar1(sender, val))?;
    if let crate::Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Map the given physical address to the given virtual address.
/// The `size` field must be page-aligned.
pub fn return_scalar2(sender: MessageSender, val1: usize, val2: usize) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnScalar2(sender, val1, val2))?;
    if let crate::Result::Ok = result {
        Ok(())
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Return 5 scalars to the provided message.
pub fn return_scalar5(
    sender: MessageSender,
    val1: usize,
    val2: usize,
    val3: usize,
    val4: usize,
    val5: usize,
) -> core::result::Result<(), Error> {
    let result = rsyscall(SysCall::ReturnScalar5(sender, val1, val2, val3, val4, val5))?;
    if let crate::Result::Ok = result {
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
        MemoryAddress::new(callback as *mut usize as usize).ok_or(Error::InvalidSyscall)?,
        MemoryAddress::new(arg as *mut usize as usize),
    ))?;
    if let crate::Result::Ok = result {
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
/// * **OutOfMemory**: No more servers may be created because the server count limit has been reached, or the
///   system does not have enough memory for the backing store.
/// * **ServerExists**: A server has already registered with that name
/// * **InvalidString**: The name was not a valid UTF-8 string
pub fn create_server_with_address(name_bytes: &[u8; 16]) -> core::result::Result<SID, Error> {
    let sid = SID::from_bytes(name_bytes).ok_or(Error::InvalidString)?;

    let result = rsyscall(SysCall::CreateServerWithAddress(sid))?;
    if let Result::NewServerID(sid, _cid) = result {
        Ok(sid)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Create a new server with the given SID.  This enables other processes to
/// connect to this server to send messages.  The name is a unique 128-bit SID.
/// That way, if a process crashes and is restarted, it can keep the same
/// name.  However, other processes cannot spoof this process.
///
/// # Errors
///
/// * **OutOfMemory**: No more servers may be created because the server count limit has been reached, or the
///   system does not have enough memory for the backing store.
/// * **ServerExists**: A server has already registered with that name
/// * **InvalidString**: The name was not a valid UTF-8 string
pub fn create_server_with_sid(sid: SID) -> core::result::Result<SID, Error> {
    let result = rsyscall(SysCall::CreateServerWithAddress(sid))?;
    if let Result::NewServerID(sid, _cid) = result {
        Ok(sid)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Create a new server with a random name.  This enables other processes to
/// connect to this server to send messages.  A random server ID is generated
/// by the kernel and returned to the caller. This address can then be registered
/// to a namserver.
///
/// # Errors
///
/// * **ServerNotFound**: No more servers may be created
/// * **OutOfMemory**: No more servers may be created because the server count limit has been reached, or the
///   system does not have enough memory for the backing store.
pub fn create_server() -> core::result::Result<SID, Error> {
    let result = rsyscall(SysCall::CreateServer)?;
    if let Result::NewServerID(sid, _cid) = result {
        Ok(sid)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Fetch a random server ID from the kernel. This is used
/// exclusively by the name server and the suspend/resume server.  A random server ID is generated
/// by the kernel and returned to the caller. This address can then be registered
/// to a namserver by the caller in their memory space.
///
/// The implementation is just a call to the kernel-exclusive TRNG to fetch random numbers.
///
/// # Errors
pub fn create_server_id() -> core::result::Result<SID, Error> {
    let result = rsyscall(SysCall::CreateServerId)?;
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

/// Connect to a server with the given SID
pub fn try_connect(server: SID) -> core::result::Result<CID, Error> {
    let result = rsyscall(SysCall::TryConnect(server))?;
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
pub fn receive_message(server: SID) -> core::result::Result<MessageEnvelope, Error> {
    let result = rsyscall(SysCall::ReceiveMessage(server)).expect("Couldn't call ReceiveMessage");
    if let Result::MessageEnvelope(envelope) = result {
        Ok(envelope)
    } else if let Result::Error(e) = result {
        Err(e)
    } else {
        Err(Error::InternalError)
    }
}

/// Retrieve a message from the message queue for the provided server. If no message
/// is available, returns `Ok(None)` without blocking
///
/// # Errors
pub fn try_receive_message(server: SID) -> core::result::Result<Option<MessageEnvelope>, Error> {
    let result = rsyscall(SysCall::TryReceiveMessage(server)).expect("Couldn't call ReceiveMessage");
    if let Result::MessageEnvelope(envelope) = result {
        Ok(Some(envelope))
    } else if result == Result::None {
        Ok(None)
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
/// * **ServerQueueFull**: The queue in the server is full, and this call would block
/// * **Timeout**: The timeout limit has been reached
pub fn try_send_message(connection: CID, message: Message) -> core::result::Result<Result, Error> {
    let result = rsyscall(SysCall::TrySendMessage(connection, message));
    match result {
        Ok(Result::Ok) => Ok(Result::Ok),
        Ok(Result::Scalar1(a)) => Ok(Result::Scalar1(a)),
        Ok(Result::Scalar2(a, b)) => Ok(Result::Scalar2(a, b)),
        Ok(Result::Scalar5(a, b, c, d, e)) => Ok(Result::Scalar5(a, b, c, d, e)),
        Ok(Result::MemoryReturned(offset, valid)) => Ok(Result::MemoryReturned(offset, valid)),
        Ok(Result::MessageEnvelope(msg)) => Ok(Result::MessageEnvelope(msg)),
        Err(e) => Err(e),
        v => panic!("Unexpected return value: {:?}", v),
    }
}

/// Connect to a server on behalf of another process. This can be used by a name
/// resolution server to securely create connections without disclosing a SID.
///
/// # Errors
///
/// * **ServerNotFound**: The server does not exist so the connection is now invalid
/// * **BadAddress**: The client tried to pass a Memory message using an address it doesn't own
/// * **ServerQueueFull**: The queue in the server is full, and this call would block
/// * **Timeout**: The timeout limit has been reached
pub fn connect_for_process(pid: PID, sid: SID) -> core::result::Result<Result, Error> {
    let result = rsyscall(SysCall::ConnectForProcess(pid, sid));
    match result {
        Ok(Result::ConnectionID(cid)) => Ok(Result::ConnectionID(cid)),
        Err(e) => Err(e),
        v => panic!("Unexpected return value: {:?}", v),
    }
}

/// Send a message to a server.  Depending on the mesage type (move or borrow), it
/// will either block (borrow) or return immediately (move).
/// If the message type is `borrow`, then the memory addresses pointed to will be
/// unavailable to this process until this function returns.
///
/// If the server queue is full, this will block.
///
/// # Errors
///
/// * **ServerNotFound**: The server does not exist so the connection is now invalid
/// * **BadAddress**: The client tried to pass a Memory message using an address it doesn't own
/// * **Timeout**: The timeout limit has been reached
pub fn send_message(connection: CID, message: Message) -> core::result::Result<Result, Error> {
    let result = rsyscall(SysCall::SendMessage(connection, message));
    match result {
        Ok(Result::Ok) => Ok(Result::Ok),
        Ok(Result::Scalar1(a)) => Ok(Result::Scalar1(a)),
        Ok(Result::Scalar2(a, b)) => Ok(Result::Scalar2(a, b)),
        Ok(Result::Scalar5(a, b, c, d, e)) => Ok(Result::Scalar5(a, b, c, d, e)),
        Ok(Result::MemoryReturned(offset, valid)) => Ok(Result::MemoryReturned(offset, valid)),
        Err(e) => Err(e),
        v => panic!("Unexpected return value: {:?}", v),
    }
}

pub fn terminate_process(exit_code: u32) -> ! {
    rsyscall(SysCall::TerminateProcess(exit_code)).expect("terminate_process returned an error");
    panic!("process didn't terminate");
}

/// Return execution to the kernel. This function may return at any time,
/// including immediately
pub fn yield_slice() { rsyscall(SysCall::Yield).ok(); }

/// Return execution to the kernel and wait for a message or an interrupt.
pub fn wait_event() { rsyscall(SysCall::WaitEvent).ok(); }

#[deprecated(since = "0.2.0", note = "Please use create_thread_n() or create_thread()")]
pub fn create_thread_simple<T, U>(
    f: fn(T) -> U,
    arg: T,
) -> core::result::Result<crate::arch::WaitHandle<U>, Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    let thread_info = crate::arch::create_thread_simple_pre(&f, &arg)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_simple_post(f, arg, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

pub fn create_thread_0<T>(f: fn() -> T) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_0_pre(&f)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_0_post(f, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

pub fn create_thread_1<T>(
    f: fn(usize) -> T,
    arg1: usize,
) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_1_pre(&f, &arg1)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_1_post(f, arg1, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

pub fn create_thread_2<T>(
    f: fn(usize, usize) -> T,
    arg1: usize,
    arg2: usize,
) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_2_pre(&f, &arg1, &arg2)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_2_post(f, arg1, arg2, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

pub fn create_thread_3<T>(
    f: fn(usize, usize, usize) -> T,
    arg1: usize,
    arg2: usize,
    arg3: usize,
) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_3_pre(&f, &arg1, &arg2, &arg3)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_3_post(f, arg1, arg2, arg3, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

pub fn create_thread_4<T>(
    f: fn(usize, usize, usize, usize) -> T,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_4_pre(&f, &arg1, &arg2, &arg3, &arg4)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_4_post(f, arg1, arg2, arg3, arg4, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

/// Create a new thread with the given closure.
pub fn create_thread<F, T>(f: F) -> core::result::Result<crate::arch::WaitHandle<T>, Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let thread_info = crate::arch::create_thread_pre(&f)?;
    rsyscall(SysCall::CreateThread(thread_info)).and_then(|result| {
        if let Result::ThreadID(thread_id) = result {
            crate::arch::create_thread_post(f, thread_id)
        } else {
            Err(Error::InternalError)
        }
    })
}

/// Wait for a thread to finish. This is equivalent to `join_thread`
pub fn wait_thread<T>(joiner: crate::arch::WaitHandle<T>) -> SysCallResult {
    crate::arch::wait_thread(joiner)
}

/// Create a new process by running it in its own thread
#[cfg(feature = "processes-as-threads")]
pub fn create_process_as_thread<F>(
    args: ProcessArgsAsThread<F>,
) -> core::result::Result<crate::arch::ProcessHandleAsThread, Error>
where
    F: FnOnce() + Send + 'static,
{
    let process_init = crate::arch::create_process_pre_as_thread(&args)?;
    rsyscall(SysCall::CreateProcess(process_init)).and_then(|result| {
        if let Result::NewProcess(startup) = result {
            crate::arch::create_process_post_as_thread(args, process_init, startup)
        } else {
            Err(Error::InternalError)
        }
    })
}

/// Wait for a thread to finish
#[cfg(feature = "processes-as-threads")]
pub fn wait_process_as_thread(joiner: crate::arch::ProcessHandleAsThread) -> SysCallResult {
    crate::arch::wait_process_as_thread(joiner)
}

pub fn create_process(args: ProcessArgs) -> core::result::Result<crate::arch::ProcessHandle, Error> {
    let process_init = crate::arch::create_process_pre(&args)?;
    rsyscall(SysCall::CreateProcess(process_init)).and_then(|result| {
        if let Result::NewProcess(startup) = result {
            crate::arch::create_process_post(args, process_init, startup)
        } else {
            Err(Error::InternalError)
        }
    })
}

/// Wait for a thread to finish
pub fn wait_process(joiner: crate::arch::ProcessHandle) -> SysCallResult { crate::arch::wait_process(joiner) }

/// Get the current process ID
pub fn current_pid() -> core::result::Result<PID, Error> {
    rsyscall(SysCall::GetProcessId).and_then(|result| {
        if let Result::ProcessID(pid) = result { Ok(pid) } else { Err(Error::InternalError) }
    })
}

/// Get the current thread ID
pub fn current_tid() -> core::result::Result<TID, Error> {
    rsyscall(SysCall::GetThreadId).and_then(|result| {
        if let Result::ThreadID(tid) = result { Ok(tid) } else { Err(Error::InternalError) }
    })
}

pub fn destroy_server(sid: SID) -> core::result::Result<(), Error> {
    rsyscall(SysCall::DestroyServer(sid))
        .and_then(|result| if let Result::Ok = result { Ok(()) } else { Err(Error::InternalError) })
}

/// Disconnect the specified connection ID and mark it as free. This
/// connection ID may be reused by the server in the future, so ensure
/// no other threads are using the connection ID before disposing of it.
///
/// # Safety
///
/// This function must only be called when the connection is no longer in
/// use. Calling this function when the connection ID is in use will result
/// in kernel errors or, if the CID is reused, silent failures due to
/// messages going to the wrong server.
pub unsafe fn disconnect(cid: CID) -> core::result::Result<(), Error> {
    rsyscall(SysCall::Disconnect(cid))
        .and_then(|result| if let Result::Ok = result { Ok(()) } else { Err(Error::InternalError) })
}

/// Block the current thread and wait for the specified thread to
/// return. Returns the return value of the thread.
///
/// # Errors
///
/// * **ThreadNotAvailable**: The thread could not be found, or was not sleeping.
pub fn join_thread(tid: TID) -> core::result::Result<usize, Error> {
    rsyscall(SysCall::JoinThread(tid)).and_then(|result| {
        if let Result::Scalar1(val) = result {
            Ok(val)
        } else if let Result::Error(Error::ThreadNotAvailable) = result {
            Err(Error::ThreadNotAvailable)
        } else {
            Err(Error::InternalError)
        }
    })
}

/// Reply to the message, if one exists, and receive the next one.
/// If no message exists, delegate the call to `receive_syscall()`.
pub fn reply_and_receive_next(
    server: SID,
    msg: &mut Option<MessageEnvelope>,
) -> core::result::Result<(), crate::Error> {
    reply_and_receive_next_legacy(server, msg, &mut 0)
}

/// Reply to the message, if one exists, and receive the next one.
/// If no message exists, delegate the call to `receive_syscall()`.
/// Allow specifying the scalar return type.
///
/// This is named `_legacy` because it is meant to work with calls that
/// expect both `Scalar1` and `Scalar2` values, in addition to `Scalar5`.
///
/// ## Arguments
///
///  * **server**: The SID of the server to receive messages from
///  * **msg**: An `Option<MessageEnvelope>` specifying the message to return.
///  * **return_type**: If 1 or 2, responds to a BlockingScalarMessage with a Scalar1 or a Scalar2. Otherwise,
///    will respond as normal.
pub fn reply_and_receive_next_legacy(
    server: SID,
    msg: &mut Option<MessageEnvelope>,
    return_type: &mut usize,
) -> core::result::Result<(), crate::Error> {
    let mut rt = *return_type;
    *return_type = 0;
    if let Some(envelope) = msg.take() {
        // If the message inside is nonblocking, then there's nothing to return.
        // Delegate reception to the `receive_message()` call
        if !envelope.body.is_blocking() {
            *msg = Some(receive_message(server)?);
            return Ok(());
        }

        let args = if let Some(mem) = envelope.body.memory_message() {
            // Allow hosted mode to detect this is a memory message by giving a sentinal value here
            rt = usize::MAX;
            mem.to_usize()
        } else if let Some(scalar) = envelope.body.scalar_message() {
            scalar.to_usize()
        } else {
            panic!("unrecognized message type")
        };
        let sender = envelope.sender;
        core::mem::forget(envelope);
        let call = SysCall::ReplyAndReceiveNext(sender, args[0], args[1], args[2], args[3], args[4], rt);
        match rsyscall(call) {
            Ok(crate::Result::MessageEnvelope(envelope)) => {
                *msg = Some(envelope);
                Ok(())
            }
            Ok(crate::Result::Error(e)) => Err(e),
            _ => Err(crate::Error::InternalError),
        }
    } else {
        // No waiting message -- call the existing `receive_message()` function
        *msg = Some(receive_message(server)?);
        Ok(())
    }
}

/* https://github.com/betrusted-io/xous-core/issues/90
static EXCEPTION_HANDLER: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
fn handle_exception(exception_type: usize, arg1: usize, arg2: usize) -> isize {
    let exception = crate::exceptions::Exception::new(exception_type, arg1, arg2);
    let f = EXCEPTION_HANDLER.load(core::sync::atomic::Ordering::SeqCst);
    let f = unsafe { core::mem::transmute::<usize, fn(Exception) -> isize>(f) };
    f(exception)
}
/// Sets the given function as this process' Exception handler. This function
/// will be called whenever an Exception occurs such as a memory fault,
/// illegal instruction, or a child process terminating.
pub fn set_exception_handler(
    handler: fn(crate::Exception) -> isize,
) -> core::result::Result<(), Error> {
    let flags = crate::MemoryFlags::R | crate::MemoryFlags::W | crate::MemoryFlags::RESERVE;

    let stack = crate::map_memory(None, None, 131_072, flags)?;
    EXCEPTION_HANDLER.store(handler as usize, core::sync::atomic::Ordering::SeqCst);
    rsyscall(SysCall::SetExceptionHandler(
        handle_exception as usize,
        stack.as_ptr() as usize,
    ))
    .and_then(|result| {
        if let Result::Ok = result {
            Ok(())
        } else if let Result::Error(Error::ThreadNotAvailable) = result {
            Err(Error::ThreadNotAvailable)
        } else {
            Err(Error::InternalError)
        }
    })
}
*/

/// Translate a virtual address to a physical address
#[cfg(feature = "v2p")]
pub fn virt_to_phys(va: usize) -> core::result::Result<usize, Error> {
    rsyscall(SysCall::VirtToPhys(va))
        .and_then(|result| if let Result::Scalar1(pa) = result { Ok(pa) } else { Err(Error::BadAddress) })
}

/// Translate a virtual address to a physical address for a given process
#[cfg(feature = "v2p")]
pub fn virt_to_phys_pid(pid: PID, va: usize) -> core::result::Result<usize, Error> {
    rsyscall(SysCall::VirtToPhysPid(pid, va))
        .and_then(|result| if let Result::Scalar1(pa) = result { Ok(pa) } else { Err(Error::BadAddress) })
}

pub fn increase_heap(bytes: usize, flags: MemoryFlags) -> core::result::Result<MemoryRange, ()> {
    let res = crate::arch::syscall(SysCall::IncreaseHeap(bytes, flags));
    if let Ok(Result::MemoryRange(range)) = res {
        return Ok(range);
    }

    Err(())
}

/// Perform a raw syscall and return the result. This will transform
/// `xous::Result::Error(e)` into an `Err(e)`.
pub fn rsyscall(call: SysCall) -> SysCallResult { crate::arch::syscall(call) }

// /// This is dangerous, but fast.
// pub unsafe fn dangerous_syscall(call: SysCall) -> SyscallResult {
//     use core::mem::{transmute, MaybeUninit};
//     let mut ret = MaybeUninit::uninit().assume_init();
//     let presto = transmute::<_, (usize, usize, usize, usize, usize, usize, usize, usize)>(call);
//     _xous_syscall_rust(
//         presto.0, presto.1, presto.2, presto.3, presto.4, presto.5, presto.6, presto.7, &mut ret,
//     );
//     match ret {
//         Result::Error(e) => Err(e),
//         other => Ok(other),
//     }
// }
