use crate::{MemoryAddress, PID, TID};
use core::convert::TryInto;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessArgs {
    name: [u8; 16],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadInit {
    call: fn(data: *const usize) -> usize,
    stack: MemoryAddress,
    arg: Option<MemoryAddress>,
    name: [u8; 16],
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessKey([u8; 16]);
impl ProcessKey {
    pub fn new(key: [u8; 16]) -> ProcessKey {
        ProcessKey(key)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessInit {
    pub key: ProcessKey,
}

pub struct WaitHandle<T>(core::marker::PhantomData<T>);
pub struct ProcessHandle(());

pub fn thread_to_args(call: usize, init: ThreadInit) -> [usize; 8] {
    [
        call as usize,
        init.call as usize,
        init.stack.get(),
        init.arg.map(|x| x.get()).unwrap_or_default(),
        0,
        0,
        0,
        0,
    ]
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
    // let call = unsafe { core::mem::transmute::<usize, fn(usize) -> usize;
    // let stack = MemoryAddress::new(a2).ok_or(Error::InvalidSyscall)?;
    // let arg = MemoryAddress::new(a2);
    // Ok(ThreadInit {
    //     call,
    //     stack,
    //     arg,
    //     name: [8u8; 16],
    // })
}

pub fn create_thread_pre<F, T>(_f: &F) -> core::result::Result<ThreadInit, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    todo!()
    // Ok(ThreadInit {})
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
    // let server_address = xous_address();
    // let server_connection =
    //     XOUS_SERVER_CONNECTION.with(|xsc| xsc.borrow().as_ref().unwrap().clone());
    // let process_id = PROCESS_ID.with(|pid| *pid.borrow());
    // Ok(std::thread::Builder::new()
    //     .spawn(move || {
    //         set_xous_address(server_address);
    //         THREAD_ID.with(|tid| *tid.borrow_mut() = thread_id);
    //         PROCESS_ID.with(|pid| *pid.borrow_mut() = process_id);
    //         XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = Some(server_connection));
    //         f()
    //     })
    //     .map(WaitHandle)
    //     .map_err(|_| crate::Error::InternalError)?)
}

pub fn wait_thread<T>(_joiner: WaitHandle<T>) -> crate::SysCallResult {
    todo!()
    // joiner
    //     .0
    //     .join()
    //     .map(|_| Result::Ok)
    //     .map_err(|_| crate::Error::InternalError)
}

pub fn process_to_args(call: usize, init: &ProcessInit) -> [usize; 8] {
    [
        call,
        u32::from_le_bytes(init.key.0[0..4].try_into().unwrap()) as _,
        u32::from_le_bytes(init.key.0[4..8].try_into().unwrap()) as _,
        u32::from_le_bytes(init.key.0[8..12].try_into().unwrap()) as _,
        u32::from_le_bytes(init.key.0[12..16].try_into().unwrap()) as _,
        0,
        0,
        0,
    ]
}

pub fn args_to_process(
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> core::result::Result<ProcessInit, crate::Error> {
    todo!()
}

pub fn create_thread_simple_pre<T, U>(
    _f: &fn(T) -> U,
    _arg: &T,
) -> core::result::Result<ThreadInit, crate::Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    todo!()
}

pub fn create_thread_simple_post<T, U>(
    _f: fn(T) -> U,
    _arg: T,
    thread_id: TID,
) -> core::result::Result<WaitHandle<T>, crate::Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    todo!()
}

/// If no connection exists, create a new connection to the server. This means
/// our parent PID will be PID1. Otherwise, reuse the same connection.
pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    todo!()
}

pub fn create_process_post(
    _args: ProcessArgs,
    _init: ProcessInit,
    _pid: PID,
) -> core::result::Result<ProcessHandle, crate::Error> {
    todo!()
}

pub fn wait_process(_joiner: ProcessHandle) -> crate::SysCallResult {
    todo!()
}
