use crate::TID;
use crate::definitions::SysCallResult;
use crate::MemoryRange;
use crate::MemoryFlags;
use crate::MemoryAddress;
use crate::SysCall;

#[derive(Debug, PartialEq, Eq)]
pub struct ProcessArgs;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInit;
impl TryFrom<[usize; 7]> for ProcessInit {
    type Error = crate::Error;
    fn try_from(_src: [usize; 7]) -> Result<ProcessInit, Self::Error> {
        todo!()
    }
}
impl Into<[usize; 7]> for &ProcessInit {
    fn into(self) -> [usize; 7] {
        todo!()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProcessStartup;
impl From<&[usize; 7]> for ProcessStartup {
    fn from(_src: &[usize; 7]) -> ProcessStartup {
        todo!()
    }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(_src: [usize; 8]) -> ProcessStartup {
        todo!()
    }
}

impl Into<[usize; 7]> for &ProcessStartup {
    fn into(self) -> [usize; 7] {
        todo!()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ProcessKey;

pub fn wait_thread<T>(_joiner: WaitHandle<T>) -> crate::SysCallResult {
    todo!()
}

pub fn syscall(_call: SysCall) -> SysCallResult {
    todo!()
}

pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    todo!()
}

pub fn create_process_post(
    _args: ProcessArgs,
    _init: ProcessInit,
    _startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    todo!();
}

pub struct WaitHandle<T>(T);
pub struct ProcessHandle;

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

pub fn create_thread_post<F, T>(
    _f: F,
    _thread_id: TID,
) -> core::result::Result<WaitHandle<T>, crate::Error>
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
    create_thread_n_pre(
        *f as usize,
        unsafe { core::mem::transmute(arg) },
        &0,
        &0,
        &0,
    )
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
    _thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    todo!()
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

pub fn unmap_memory_pre(_range: &MemoryRange) -> core::result::Result<(), crate::Error> {
    Ok(())
}

pub fn unmap_memory_post(_range: MemoryRange) -> core::result::Result<(), crate::Error> {
    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ThreadInit;

impl ThreadInit {
    pub fn new(
        _call: usize,
        _stack: MemoryRange,
        _arg1: usize,
        _arg2: usize,
        _arg3: usize,
        _arg4: usize,
    ) -> Self {
        todo!()
    }
}

/// This code is executed inside the kernel. It takes the list of args
/// that were passed via registers and converts them into a `ThreadInit`
/// struct with enough information to start the new thread.
pub fn args_to_thread(
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> core::result::Result<ThreadInit, crate::Error> {
    todo!()
}

pub fn thread_to_args(_syscall: usize, _init: &ThreadInit) -> [usize; 8] {
    todo!();
}