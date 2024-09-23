use crate::MemoryAddress;
use crate::MemoryFlags;
use crate::MemoryRange;
use crate::TID;
use crate::definitions::SysCallResult;

pub mod irq;
mod syscall;
pub use syscall::*;
pub mod process;
pub use process::*;

pub struct ProcessHandle {
    pub pid: crate::PID,
    pub cid: crate::CID,
}

pub fn wait_thread<T>(_joiner: WaitHandle<T>) -> crate::SysCallResult { todo!() }

pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> { todo!() }

pub fn create_process_post(
    _args: ProcessArgs,
    _init: ProcessInit,
    _startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    todo!();
}

pub struct WaitHandle<T> {
    #[allow(dead_code)]
    tid: TID,
    data: core::marker::PhantomData<T>,
}

pub fn wait_process(_joiner: crate::arch::ProcessHandle) -> SysCallResult {
    todo!();
}

pub fn create_thread_pre<F, T>(_f: &F) -> core::result::Result<ThreadInit, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    todo!()
}

pub fn create_thread_post<F, T>(_f: F, _thread_id: TID) -> core::result::Result<WaitHandle<T>, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    todo!()
}

pub fn create_thread_0_pre<U>(f: &fn() -> U) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    let start = *f as usize;
    create_thread_n_pre(start, &0, &0, &0, &0)
}

pub fn create_thread_0_post<U>(
    f: fn() -> U,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    let start = f as usize;
    create_thread_n_post(start, 0, 0, 0, 0, thread_id)
}

pub fn create_thread_1_pre<U>(
    f: &fn(usize) -> U,
    arg1: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    let start = *f as usize;
    create_thread_n_pre(start, arg1, &0, &0, &0)
}

pub fn create_thread_1_post<U>(
    f: fn(usize) -> U,
    arg1: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    let start = f as usize;
    create_thread_n_post(start, arg1, 0, 0, 0, thread_id)
}

pub fn create_thread_2_pre<U>(
    f: &fn(usize, usize) -> U,
    arg1: &usize,
    arg2: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    let start = *f as usize;
    create_thread_n_pre(start, arg1, arg2, &0, &0)
}

pub fn create_thread_2_post<U>(
    f: fn(usize, usize) -> U,
    arg1: usize,
    arg2: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    let start = f as usize;
    create_thread_n_post(start, arg1, arg2, 0, 0, thread_id)
}

pub fn create_thread_3_pre<U>(
    f: &fn(usize, usize, usize) -> U,
    arg1: &usize,
    arg2: &usize,
    arg3: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    let start = *f as usize;
    create_thread_n_pre(start, arg1, arg2, arg3, &0)
}

pub fn create_thread_3_post<U>(
    f: fn(usize, usize, usize) -> U,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    let start = f as usize;
    create_thread_n_post(start, arg1, arg2, arg3, 0, thread_id)
}

pub fn create_thread_4_pre<U>(
    f: &fn(usize, usize, usize, usize) -> U,
    arg1: &usize,
    arg2: &usize,
    arg3: &usize,
    arg4: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    let start = *f as usize;
    create_thread_n_pre(start, arg1, arg2, arg3, arg4)
}

pub fn create_thread_4_post<U>(
    f: fn(usize, usize, usize, usize) -> U,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    let start = f as usize;
    create_thread_n_post(start, arg1, arg2, arg3, arg4, thread_id)
}

pub fn create_thread_simple_pre<T, U>(
    f: &fn(T) -> U,
    arg: &T,
) -> core::result::Result<ThreadInit, crate::Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    create_thread_n_pre(*f as usize, unsafe { core::mem::transmute(arg) }, &0, &0, &0)
}

pub fn create_thread_simple_post<T, U>(
    f: fn(T) -> U,
    arg: T,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    create_thread_n_post(
        f as usize,
        unsafe { core::mem::transmute(&arg) },
        0,
        0,
        0,
        thread_id,
    )
    // If we succeeded, the variable will be moved into the caller. Drop it from here.
    .map(|f| {
        core::mem::forget(arg);
        f
    })
}

pub fn create_thread_n_pre(
    start: usize,
    arg1: &usize,
    arg2: &usize,
    arg3: &usize,
    arg4: &usize,
) -> core::result::Result<ThreadInit, crate::Error> {
    let flags = crate::MemoryFlags::R | crate::MemoryFlags::W | crate::MemoryFlags::RESERVE;

    let stack = crate::map_memory(None, None, 131_072, flags)?;
    Ok(ThreadInit::new(start, stack, *arg1, *arg2, *arg3, *arg4))
}

pub fn create_thread_n_post<U>(
    _f: usize,
    _arg1: usize,
    _arg2: usize,
    _arg3: usize,
    _arg4: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    Ok(WaitHandle { tid: thread_id, data: core::marker::PhantomData })
}

pub fn map_memory_pre(
    _phys: &Option<MemoryAddress>,
    _virt: &Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
) -> core::result::Result<(), crate::Error> {
    Ok(())
}

pub fn map_memory_post(
    _phys: Option<MemoryAddress>,
    _virt: Option<MemoryAddress>,
    _size: usize,
    _flags: MemoryFlags,
    range: MemoryRange,
) -> core::result::Result<MemoryRange, crate::Error> {
    Ok(range)
}

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), crate::Error> { Ok(()) }

pub fn unmap_memory_post(_range: MemoryRange) -> core::result::Result<(), crate::Error> { Ok(()) }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadInit {
    /// Function pointer that accepts 0-4 arguments
    pub call: usize,
    pub stack: MemoryRange,
    pub arg1: usize,
    pub arg2: usize,
    pub arg3: usize,
    pub arg4: usize,
}

impl ThreadInit {
    pub fn new(call: usize, stack: MemoryRange, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> Self {
        ThreadInit { call, stack, arg1, arg2, arg3, arg4 }
    }
}

impl Default for ThreadInit {
    fn default() -> Self {
        ThreadInit {
            call: 1,
            stack: unsafe { MemoryRange::new(4, 4).unwrap() },
            arg1: 0,
            arg2: 0,
            arg3: 0,
            arg4: 0,
        }
    }
}

/// This code is executed inside the kernel. It takes the list of args
/// that were passed via registers and converts them into a `ThreadInit`
/// struct with enough information to start the new thread.
pub fn args_to_thread(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> core::result::Result<ThreadInit, crate::Error> {
    Ok(ThreadInit {
        call: a1,
        stack: unsafe { MemoryRange::new(a2, a3).map_err(|_| crate::Error::InvalidSyscall) }?,
        arg1: a4,
        arg2: a5,
        arg3: a6,
        arg4: a7,
    })
}

pub fn thread_to_args(syscall: usize, init: &ThreadInit) -> [usize; 8] {
    [
        syscall,
        init.call,
        init.stack.as_ptr() as _,
        init.stack.len(),
        init.arg1,
        init.arg2,
        init.arg3,
        init.arg4,
    ]
}
