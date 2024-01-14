use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread_local;

use crate::{Result, TID};

thread_local!(pub static THREAD_ID: RefCell<Option<TID>> = RefCell::new(None));

/// Describes the parameters required to create a new thread on this platform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadInit {}
pub struct WaitHandle<T>(std::thread::JoinHandle<T>);

pub fn thread_to_args(call: usize, _init: &ThreadInit) -> [usize; 8] { [call, 0, 0, 0, 0, 0, 0, 0] }

pub fn args_to_thread(
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> core::result::Result<ThreadInit, crate::Error> {
    Ok(ThreadInit {})
}

pub fn create_thread_0_pre<U>(_f: &fn() -> U) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    Ok(ThreadInit {})
}
pub fn create_thread_1_pre<U>(
    _f: &fn(usize) -> U,
    _arg1: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    Ok(ThreadInit {})
}
pub fn create_thread_2_pre<U>(
    _f: &fn(usize, usize) -> U,
    _arg1: &usize,
    _arg2: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    Ok(ThreadInit {})
}
pub fn create_thread_3_pre<U>(
    _f: &fn(usize, usize, usize) -> U,
    _arg1: &usize,
    _arg2: &usize,
    _arg3: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    Ok(ThreadInit {})
}
pub fn create_thread_4_pre<U>(
    _f: &fn(usize, usize, usize, usize) -> U,
    _arg1: &usize,
    _arg2: &usize,
    _arg3: &usize,
    _arg4: &usize,
) -> core::result::Result<ThreadInit, crate::Error>
where
    U: Send + 'static,
{
    Ok(ThreadInit {})
}

pub fn create_thread_0_post<U>(
    f: fn() -> U,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    create_thread_post(f, thread_id)
}

pub fn create_thread_1_post<U>(
    f: fn(usize) -> U,
    arg1: usize,
    thread_id: TID,
) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    U: Send + 'static,
{
    create_thread_post(move || f(arg1), thread_id)
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
    create_thread_post(move || f(arg1, arg2), thread_id)
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
    create_thread_post(move || f(arg1, arg2, arg3), thread_id)
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
    create_thread_post(move || f(arg1, arg2, arg3, arg4), thread_id)
}

pub fn create_thread_simple_pre<T, U>(
    _f: &fn(T) -> U,
    _arg: &T,
) -> core::result::Result<ThreadInit, crate::Error>
where
    T: Send + 'static,
    U: Send + 'static,
{
    Ok(ThreadInit {})
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
    create_thread_post(move || f(arg), thread_id)
}

pub fn create_thread_pre<F, T>(_f: &F) -> core::result::Result<ThreadInit, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    Ok(ThreadInit {})
}

/// Spawn a new thread with the given thread ID.
pub fn create_thread_post<F, U>(f: F, thread_id: TID) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    F: FnOnce() -> U,
    F: Send + 'static,
    U: Send + 'static,
{
    std::thread::Builder::new()
        .spawn(move || {
            THREAD_ID.with(|tid| *tid.borrow_mut() = Some(thread_id));
            f()
        })
        .map(WaitHandle)
        .map_err(|_| crate::Error::InternalError)
}

pub fn wait_thread<T>(joiner: WaitHandle<T>) -> crate::SysCallResult {
    joiner.0.join().map(|_| Result::Ok).map_err(|_| crate::Error::InternalError)
}

static FAKE_THREAD_COUNTER: AtomicUsize = AtomicUsize::new(65536);

/// Obtain this thread's ID. This value is set whenever a new thread is
/// created via xous syscalls. However, if a new thread is created using
/// something other than a xous syscall (e.g. by doing `std::thread::spawn()),
/// then the kernel doesn't have a record of the new thread.
/// If that happens, then register a new thread with the kernel.
pub(crate) fn thread_id() -> TID {
    THREAD_ID.with(|tid| {
        if let Some(tid) = *tid.borrow() {
            return tid;
        }
        let call = crate::SysCall::CreateThread(ThreadInit {});

        let fake_tid = FAKE_THREAD_COUNTER.fetch_add(1, Ordering::SeqCst);
        // println!(
        //     "Thread ID not defined! Creating a fake thread. Syscall TID: {}.",
        //     fake_tid
        // );

        // assert!(SERVER_CONNECTION
        //     .call_tracker
        //     .lock()
        //     .unwrap()
        //     .insert(fake_tid, ())
        //     .is_none());

        super::send_syscall_from_tid(&call, fake_tid);
        let response = super::read_syscall_result(fake_tid);

        // assert!(SERVER_CONNECTION
        //     .response_tracker
        //     .lock()
        //     .unwrap()
        //     .remove(&fake_tid)
        //     .is_some());
        let new_tid = if let crate::Result::ThreadID(new_tid) = response {
            new_tid
        } else {
            panic!("unable to get new TID, got response: {:?}", response)
        };
        // println!("Created a fake thread ID: {}", new_tid);
        *tid.borrow_mut() = Some(new_tid);
        new_tid
    })
}
