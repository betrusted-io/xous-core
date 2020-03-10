use crate::{CpuID, Error, PID};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;

bitflags! {
    /// Flags to be passed to the MapMemory struct.
    /// Note that it is an error to have memory be
    /// writable and not readable.
    pub struct MemoryFlags: usize {
        /// Free this memory
        const FREE      = 0b00000000;

        /// Immediately allocate this memory.  Otherwise it will
        /// be demand-paged.  This is implicitly set when `phys`
        /// is not 0.
        const RESERVE   = 0b00000001;

        /// Allow the CPU to read from this page.
        const R         = 0b00000010;

        /// Allow the CPU to write to this page.
        const W         = 0b00000100;

        /// Allow the CPU to execute from this page.
        const X         = 0b00001000;
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

#[derive(Debug)]
pub enum SysCall {
    /// Allocates pages of memory, equal to a total of `size`
    /// bytes.  A physical address may be specified, which can be
    /// used to allocate regions such as memory-mapped I/O.
    ///
    /// If a virtual address is specified, then the returned
    /// pages are located at that address.  Otherwise, they
    /// are located at the Default offset.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned,
    ///                     or the size isn't a multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's
    ///                    memory size has been exceeded.
    MapPhysical(
        *mut usize,  /* phys */
        *mut usize,  /* virt */
        usize,       /* region size */
        MemoryFlags, /* flags */
    ),

    /// Sets the offset and size of a given memory region.  This call may only be made
    /// by processes that have not yet started, or processes that have a PPID of 1.
    /// Care must be taken to ensure this region doesn't run into other regions.
    /// Additionally, the base address must avoid the kernel regions.
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned,
    ///                     or the size isn't a multiple of the page width.
    /// * **BadAddress**: The address conflicts with the kernel
    SetMemRegion(
        PID,    /* pid */
        MemoryType, /* region type */
        *mut usize, /* region address */
        usize,      /* region size */
    ),

    /// Add the given number of bytes to the heap.  The number of bytes
    /// must be divisible by the page size.  The newly-allocated pages
    /// will have the specified flags.  To get the current heap base,
    /// call this with a size of `0`.
    ///
    /// # Returns
    ///
    /// * **MemoryRange(*mut usize /* The base of the heap */, usize /* the new size of the heap */)
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned,
    ///                     or the size isn't a multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's
    ///                    memory size has been exceeded.
    IncreaseHeap(usize /* number of bytes to add */, MemoryFlags),

    /// Remove the given number of bytes from the heap.
    ///
    /// # Returns
    ///
    /// * **MemoryRange(*mut usize /* The base of the heap */, usize /* the new size of the heap */)
    ///
    /// # Errors
    ///
    /// * **BadAlignment**: Either the physical or virtual addresses aren't page-aligned,
    ///                     or the size isn't a multiple of the page width.
    /// * **OutOfMemory**: A contiguous chunk of memory couldn't be found, or the system's
    ///                    memory size has been exceeded.
    DecreaseHeap(usize /* desired heap size */),

    /// Set the specified flags on the virtual address range.
    /// This can be used to REMOVE flags on a memory region, for example
    /// to mark it as no-execute after writing program data.
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

    /// Stop running the given process.
    Suspend(PID, CpuID),

    /// Claims an interrupt and unmasks it immediately.  The provided function will
    /// be called from within an interrupt context, but using the ordinary privilege level of
    /// the process.
    ///
    /// # Errors
    ///
    /// * **InterruptNotFound**: The specified interrupt isn't valid on this system
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
    /// * **InterruptNotFound**: The specified interrupt doesn't exist, or isn't assigned
    ///                          to this process.
    FreeInterrupt(usize /* IRQ number */),

    /// Resumes a process using the given context.  A parent could use
    /// this function to implement multi-threading inside a child process, or
    /// to create a task switcher.
    ///
    /// To resume a process exactly where it left off, set `stack_pointer` to `None`.
    /// This would be done in a very simple system that has no threads.
    ///
    /// By default, at most three context switches can be made before the quantum
    /// expires.  To enable more, pass `additional_contexts`.
    ///
    /// If no more contexts are available when one is required, then the child
    /// automatically relinquishes its quantum.
    ///
    /// # Returns
    ///
    /// When this function returns, it provides a list of the processes and
    /// stack pointers that are ready to be run.  Three can fit as return values,
    /// and additional context switches will be supplied in the slice of context
    /// switches, if one is provided.
    ///
    /// # Examples
    ///
    /// If a process called `yield()`, or if its quantum expired normally, then
    /// a single context is returned: The target thread, and its stack pointer.
    ///
    /// If the child process called `client_send()` and ended up blocking due to
    /// the server not being ready, then this would return no context switches.
    /// This thread or process should not be scheduled to run.
    ///
    /// If the child called `client_send()` and the server was ready, then the
    /// server process would be run immediately.  If the child process' quantum
    /// expired while the server was running, then this function would return
    /// a single context containing the PID of the server, and the stack pointer.
    ///
    /// If the child called `client_send()` and the server was ready, then the
    /// server process would be run immediately.  If the server then finishes,
    /// execution flow is returned to the child process.  If the quantum then
    /// expires, this would return two contexts: the server's PID and its stack
    /// pointer when it called `client_reply()`, and the child's PID with its
    /// current stack pointer.
    ///
    /// If the server in turn called another server, and both servers ended up
    /// returning to the child before the quantum expired, then there would be
    /// three contexts on the stack.
    ///
    /// # Errors
    ///
    /// * **ProcessNotFound**: The requested process does not exist
    /// * **ProcessNotChild**: The given process was not a child process, and
    ///                        therefore couldn't be resumed.
    /// * **ProcessTerminated**: The process has crashed.
    SwitchTo(PID, usize /* thread ID */),

    Invalid(usize, usize, usize, usize, usize, usize, usize),
}

#[derive(FromPrimitive)]
enum SysCallNumber {
    MapPhysical = 2,
    Yield = 3,
    Suspend = 4,
    ClaimInterrupt = 5,
    FreeInterrupt = 6,
    SwitchTo = 7,
    WaitEvent = 9,
    IncreaseHeap = 10,
    DecreaseHeap = 11,
    UpdateMemoryFlags = 12,
    SetMemRegion = 13,
    Invalid,
}

#[derive(Debug)]
pub struct InvalidSyscall {}

impl SysCall {
    pub fn as_args(&self) -> [usize; 8] {
        use core::mem;
        assert!(
            mem::size_of::<SysCall>() == mem::size_of::<usize>() * 8,
            "SysCall is not the expected size"
        );
        match *self {
            SysCall::MapPhysical(a1, a2, a3, a4) => [
                SysCallNumber::MapPhysical as usize,
                a1 as usize,
                a2 as usize,
                a3,
                a4.bits(),
                0,
                0,
                0,
            ],
            SysCall::Yield => [SysCallNumber::Yield as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::WaitEvent => [SysCallNumber::WaitEvent as usize, 0, 0, 0, 0, 0, 0, 0],
            SysCall::Suspend(a1, a2) => [
                SysCallNumber::Suspend as usize,
                a1 as usize,
                a2 as usize,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::ClaimInterrupt(a1, a2, a3) => [
                SysCallNumber::ClaimInterrupt as usize,
                a1,
                a2 as usize,
                a3 as usize,
                0,
                0,
                0,
                0,
            ],
            SysCall::FreeInterrupt(a1) => {
                [SysCallNumber::FreeInterrupt as usize, a1, 0, 0, 0, 0, 0, 0]
            }
            SysCall::SwitchTo(a1, a2) => [
                SysCallNumber::SwitchTo as usize,
                a1 as usize,
                a2 as usize,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::IncreaseHeap(a1, a2) => [
                SysCallNumber::IncreaseHeap as usize,
                a1 as usize,
                a2.bits(),
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::DecreaseHeap(a1) => [
                SysCallNumber::DecreaseHeap as usize,
                a1 as usize,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
            SysCall::UpdateMemoryFlags(a1, a2, a3) => [
                SysCallNumber::UpdateMemoryFlags as usize,
                a1 as usize,
                a2 as usize,
                a3.bits(),
                0,
                0,
                0,
                0,
            ],
            SysCall::SetMemRegion(a1, a2, a3, a4) => [
                SysCallNumber::SetMemRegion as usize,
                a1 as usize,
                a2 as usize,
                a3 as usize,
                a4,
                0,
                0,
                0,
            ],

            SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7) => {
                [SysCallNumber::Invalid as usize, a1, a2, a3, a4, a5, a6, a7]
            }
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
    ) -> core::result::Result<Self, InvalidSyscall> {
        Ok(match FromPrimitive::from_usize(a0) {
            Some(SysCallNumber::MapPhysical) => SysCall::MapPhysical(
                a1 as *mut usize,
                a2 as *mut usize,
                a3,
                MemoryFlags::from_bits(a4).ok_or(InvalidSyscall {})?,
            ),
            Some(SysCallNumber::Yield) => SysCall::Yield,
            Some(SysCallNumber::WaitEvent) => SysCall::WaitEvent,
            Some(SysCallNumber::Suspend) => SysCall::Suspend(a1 as PID, a2),
            Some(SysCallNumber::ClaimInterrupt) => {
                SysCall::ClaimInterrupt(a1, a2 as *mut usize, a3 as *mut usize)
            }
            Some(SysCallNumber::FreeInterrupt) => SysCall::FreeInterrupt(a1),
            Some(SysCallNumber::SwitchTo) => SysCall::SwitchTo(a1 as PID, a2 as usize),
            Some(SysCallNumber::IncreaseHeap) => SysCall::IncreaseHeap(
                a1 as usize,
                MemoryFlags::from_bits(a2).ok_or(InvalidSyscall {})?,
            ),
            Some(SysCallNumber::DecreaseHeap) => SysCall::DecreaseHeap(a1 as usize),
            Some(SysCallNumber::UpdateMemoryFlags) => SysCall::UpdateMemoryFlags(
                a1 as *mut usize,
                a2 as usize,
                MemoryFlags::from_bits(a3).ok_or(InvalidSyscall {})?,
            ),
            Some(SysCallNumber::SetMemRegion) => {
                SysCall::SetMemRegion(a1 as PID, MemoryType::from(a2), a3 as *mut usize, a4)
            }
            Some(SysCallNumber::Invalid) => SysCall::Invalid(a1, a2, a3, a4, a5, a6, a7),
            None => return Err(InvalidSyscall {}),
        })
    }
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum Result {
    ReturnResult,
    Error(Error),
    MemoryAddress(*mut u8),
    MemoryRange(*mut u8 /* base */, usize /* size */),
    ResumeResult(usize, usize, usize, usize, usize, usize),
    ResumeProcess,
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
