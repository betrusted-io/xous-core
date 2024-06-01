use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread_local;

use crate::{Result, SysCall, SysCallResult, PID, TID};

mod mem;
pub use mem::*;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessKey([u8; 16]);
impl ProcessKey {
    pub fn new(key: [u8; 16]) -> ProcessKey { ProcessKey(key) }
}

impl core::fmt::Display for ProcessKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for i in self.0 {
            write!(f, "{:02x}", i)?;
        }

        Ok(())
    }
}

impl From<&str> for ProcessKey {
    fn from(v: &str) -> ProcessKey {
        let mut key = [0u8; 16];
        for (src, dest) in v.as_bytes().chunks(2).zip(key.iter_mut()) {
            *dest = u8::from_str_radix(core::str::from_utf8(src).unwrap(), 16).unwrap();
        }
        ProcessKey::new(key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadInit {}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ProcessInit {
    pub key: ProcessKey,
}

pub struct ProcessArgsAsThread<F: FnOnce()> {
    main: F,
    name: String,
}

impl<F> ProcessArgsAsThread<F>
where
    F: FnOnce(),
{
    pub fn new(name: &str, main: F) -> ProcessArgsAsThread<F> {
        ProcessArgsAsThread { main, name: name.to_owned() }
    }
}
pub struct ProcessHandleAsThread(std::thread::JoinHandle<()>);

/// If no connection exists, create a new connection to the server. This means
/// our parent PID will be PID1. Otherwise, reuse the same connection.
pub fn create_process_pre_as_thread<F>(
    _args: &ProcessArgsAsThread<F>,
) -> core::result::Result<ProcessInit, crate::Error>
where
    F: FnOnce(),
{
    ensure_connection()?;

    // Ensure there is a connection, because after this function returns
    // we'll make a syscall with CreateProcess(). This should only need
    // to happen for PID1.
    Ok(ProcessInit { key: PROCESS_KEY.with(|pk| *pk.borrow()).unwrap_or_else(default_process_key) })
}

pub fn create_process_post_as_thread<F>(
    args: ProcessArgsAsThread<F>,
    init: ProcessInit,
    startup: ProcessStartup,
) -> core::result::Result<ProcessHandleAsThread, crate::Error>
where
    F: FnOnce() + Send + 'static,
{
    let server_address = xous_address();
    let pid = startup.pid;

    let f = args.main;
    let thread_main = std::thread::Builder::new()
        .name(args.name)
        .spawn(move || {
            set_xous_address(server_address);
            THREAD_ID.with(|tid| *tid.borrow_mut() = 1);
            PROCESS_ID.with(|p| *p.borrow_mut() = pid);
            XOUS_SERVER_CONNECTION.with(|xsc| {
                let mut xsc = xsc.borrow_mut();
                match xous_connect_impl(server_address, &init.key) {
                    Ok(a) => {
                        *xsc = Some(a);
                        Ok(())
                    }
                    Err(_) => Err(crate::Error::InternalError),
                }
            })?;

            crate::create_thread(f)
        })
        .map_err(|_| crate::Error::InternalError)?
        .join()
        .unwrap()
        .unwrap();

    Ok(ProcessHandleAsThread(thread_main.0))
}

pub fn wait_process_as_thread(joiner: ProcessHandleAsThread) -> crate::SysCallResult {
    joiner.0.join().map(|_| Result::Ok).map_err(|_x| {
        // panic!("wait error: {:?}", x);
        crate::Error::InternalError
    })
}

pub struct ProcessArgs {
    command: String,
    name: String,
}

impl ProcessArgs {
    pub fn new(name: &str, command: String) -> ProcessArgs { ProcessArgs { command, name: name.to_owned() } }
}

/// This is returned when a process is created
#[derive(Debug, PartialEq)]
pub struct ProcessStartup {
    /// The process ID of the new process
    pid: crate::PID,
}

impl ProcessStartup {
    pub fn new(pid: crate::PID) -> Self { ProcessStartup { pid } }

    pub fn pid(&self) -> crate::PID { self.pid }
}

impl core::fmt::Display for ProcessStartup {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { write!(f, "{}", self.pid) }
}

impl From<&[usize; 7]> for ProcessStartup {
    fn from(src: &[usize; 7]) -> ProcessStartup {
        ProcessStartup { pid: crate::PID::new(src[0] as _).unwrap() }
    }
}

impl From<[usize; 8]> for ProcessStartup {
    fn from(src: [usize; 8]) -> ProcessStartup {
        let pid = crate::PID::new(src[1] as _).unwrap();
        ProcessStartup { pid }
    }
}

impl From<&ProcessStartup> for [usize; 7] {
    fn from(startup: &ProcessStartup) -> [usize; 7] { [startup.pid.get() as _, 0, 0, 0, 0, 0, 0] }
}

impl From<&ProcessInit> for [usize; 7] {
    fn from(init: &ProcessInit) -> [usize; 7] {
        [
            u32::from_le_bytes(init.key.0[0..4].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[4..8].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[8..12].try_into().unwrap()) as _,
            u32::from_le_bytes(init.key.0[12..16].try_into().unwrap()) as _,
            0,
            0,
            0,
        ]
    }
}

impl TryFrom<[usize; 7]> for ProcessInit {
    type Error = crate::Error;

    fn try_from(src: [usize; 7]) -> core::result::Result<ProcessInit, crate::Error> {
        let mut exploded = vec![];
        for word in src[0..4].into_iter() {
            exploded.extend_from_slice(&(*word as u32).to_le_bytes());
        }
        let mut key = [0u8; 16];
        key.copy_from_slice(&exploded);
        Ok(ProcessInit { key: ProcessKey(key) })
    }
}

#[derive(Debug)]
pub struct ProcessHandle(std::process::Child);

/// If no connection exists, create a new connection to the server. This means
/// our parent PID will be PID1. Otherwise, reuse the same connection.
pub fn create_process_pre(_args: &ProcessArgs) -> core::result::Result<ProcessInit, crate::Error> {
    ensure_connection()?;

    // Ensure there is a connection, because after this function returns
    // we'll make a syscall with CreateProcess(). This should only need
    // to happen for PID1.
    Ok(ProcessInit { key: PROCESS_KEY.with(|pk| *pk.borrow()).unwrap_or_else(default_process_key) })
}

pub fn create_process_post(
    args: ProcessArgs,
    init: ProcessInit,
    startup: ProcessStartup,
) -> core::result::Result<ProcessHandle, crate::Error> {
    use std::process::Command;
    let server_env = format!("{}", xous_address());
    let pid_env = format!("{}", startup.pid);
    let process_name_env = args.name.to_string();
    let process_key_env: String = format!("{}", init.key);
    let (shell, args) = if cfg!(windows) {
        ("cmd", ["/C", &args.command])
    } else if cfg!(unix) {
        ("sh", ["-c", &args.command])
    } else {
        panic!("unrecognized platform -- don't know how to shell out");
    };

    // println!("Launching process...");
    Command::new(shell)
        .args(&args)
        .env("XOUS_SERVER", server_env)
        .env("XOUS_PID", pid_env)
        .env("XOUS_PROCESS_NAME", process_name_env)
        .env("XOUS_PROCESS_KEY", process_key_env)
        .spawn()
        .map(ProcessHandle)
        .map_err(|_| {
            // eprintln!("couldn't start command: {}", e);
            crate::Error::InternalError
        })
}

pub fn wait_process(mut joiner: ProcessHandle) -> crate::SysCallResult {
    joiner
        .0
        .wait()
        .or(Err(crate::Error::InternalError))
        .and_then(|e| if e.success() { Ok(crate::Result::Ok) } else { Err(crate::Error::UnknownError) })
}

pub struct WaitHandle<T>(std::thread::JoinHandle<T>);

#[derive(Clone)]
struct ServerConnection {
    send: Arc<Mutex<TcpStream>>,
    recv: Arc<Mutex<TcpStream>>,
    mailbox: Arc<Mutex<HashMap<TID, Result>>>,
}

pub fn thread_to_args(call: usize, _init: &ThreadInit) -> [usize; 8] { [call, 0, 0, 0, 0, 0, 0, 0] }

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

pub fn args_to_process(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> core::result::Result<ProcessInit, crate::Error> {
    let mut v = vec![];
    v.extend_from_slice(&(a1 as u32).to_le_bytes());
    v.extend_from_slice(&(a2 as u32).to_le_bytes());
    v.extend_from_slice(&(a3 as u32).to_le_bytes());
    v.extend_from_slice(&(a4 as u32).to_le_bytes());
    let mut key = [0u8; 16];
    key.copy_from_slice(&v);
    Ok(ProcessInit { key: ProcessKey(key) })
}

thread_local!(static NETWORK_CONNECT_ADDRESS: RefCell<Option<SocketAddr>> = RefCell::new(None));
thread_local!(static XOUS_SERVER_CONNECTION: RefCell<Option<ServerConnection>> = RefCell::new(None));
thread_local!(static THREAD_ID: RefCell<TID> = RefCell::new(1));
thread_local!(static PROCESS_ID: RefCell<PID> = RefCell::new(PID::new(1).unwrap()));
thread_local!(static PROCESS_KEY: RefCell<Option<ProcessKey>> = RefCell::new(None));
thread_local!(static CALL_FOR_THREAD: RefCell<Arc<Mutex<HashMap<TID, crate::SysCall>>>> = RefCell::new(Arc::new(Mutex::new(HashMap::new()))));

fn default_xous_address() -> SocketAddr {
    std::env::var("XOUS_SERVER")
        .map(|s| {
            s.to_socket_addrs()
                .expect("invalid server address")
                .next()
                .expect("unable to resolve server address")
        })
        .unwrap_or_else(|_| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0))
}

fn default_process_key() -> ProcessKey {
    std::env::var("XOUS_PROCESS_KEY").map(|s| s.as_str().into()).unwrap_or(ProcessKey([0u8; 16]))
}

pub fn set_process_key(new_key: &[u8; 16]) {
    PROCESS_KEY.with(|pk| *pk.borrow_mut() = Some(ProcessKey(*new_key)));
}

/// Set the network address for this particular thread.
pub fn set_xous_address(new_address: SocketAddr) {
    NETWORK_CONNECT_ADDRESS.with(|nca| {
        let mut address = nca.borrow_mut();
        *address = Some(new_address);
        XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = None);
    });
}

/// Get the network address for this particular thread.
fn xous_address() -> SocketAddr {
    NETWORK_CONNECT_ADDRESS.with(|nca| *nca.borrow()).unwrap_or_else(default_xous_address)
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
    create_thread_post(move || f(), thread_id)
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

pub fn create_thread_post<F, U>(f: F, thread_id: TID) -> core::result::Result<WaitHandle<U>, crate::Error>
where
    F: FnOnce() -> U,
    F: Send + 'static,
    U: Send + 'static,
{
    let server_address = xous_address();
    let server_connection = XOUS_SERVER_CONNECTION.with(|xsc| xsc.borrow().as_ref().unwrap().clone());
    let process_id = PROCESS_ID.with(|pid| *pid.borrow());
    let call_for_thread = CALL_FOR_THREAD.with(|cft| cft.borrow().clone());
    Ok(std::thread::Builder::new()
        .spawn(move || {
            set_xous_address(server_address);
            THREAD_ID.with(|tid| *tid.borrow_mut() = thread_id);
            PROCESS_ID.with(|pid| *pid.borrow_mut() = process_id);
            XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = Some(server_connection));
            CALL_FOR_THREAD.with(|cft| *cft.borrow_mut() = call_for_thread);
            f()
        })
        .map(WaitHandle)
        .map_err(|_| crate::Error::InternalError)?)
}

pub fn wait_thread<T>(joiner: WaitHandle<T>) -> crate::SysCallResult {
    joiner.0.join().map(|_| Result::Ok).map_err(|_| crate::Error::InternalError)
}

pub fn ensure_connection() -> core::result::Result<(), crate::Error> {
    XOUS_SERVER_CONNECTION.with(|xsc| {
        let mut xsc = xsc.borrow_mut();
        if xsc.is_none() {
            NETWORK_CONNECT_ADDRESS.with(|nca| {
                let addr = nca.borrow().unwrap_or_else(default_xous_address);
                let pid1_key = PROCESS_KEY.with(|pk| *pk.borrow()).unwrap_or_else(default_process_key);
                match xous_connect_impl(addr, &pid1_key) {
                    Ok(a) => {
                        *xsc = Some(a);
                        Ok(())
                    }
                    Err(_) => Err(crate::Error::InternalError),
                }
            })
        } else {
            Ok(())
        }
    })
}

fn xous_connect_impl(addr: SocketAddr, key: &ProcessKey) -> core::result::Result<ServerConnection, ()> {
    // eprintln!("Opening connection to Xous server @ {} with key {:?}...", addr, key);
    assert_ne!(&key.0, &[0u8; 16]);
    match TcpStream::connect(addr) {
        Ok(mut conn) => {
            conn.write_all(&key.0).unwrap(); // Send key to authenticate us as PID 1
            conn.flush().unwrap();
            conn.set_nodelay(true).unwrap();
            let mut pid = [0u8];
            conn.read_exact(&mut pid).unwrap();
            PROCESS_ID.with(|process_id| *process_id.borrow_mut() = PID::new(pid[0]).unwrap());
            Ok(ServerConnection {
                send: Arc::new(Mutex::new(conn.try_clone().unwrap())),
                recv: Arc::new(Mutex::new(conn)),
                mailbox: Arc::new(Mutex::new(HashMap::new())),
            })
        }
        Err(_e) => {
            // eprintln!("Unable to connect to Xous server: {}", _e);
            // eprintln!(
            //     "Ensure Xous is running, or specify this process as an argument to the kernel"
            // );
            Err(())
        }
    }
}

pub fn syscall(call: SysCall) -> SysCallResult {
    let mut ret = Result::Ok;
    XOUS_SERVER_CONNECTION.with(|xsc| {
        THREAD_ID.with(|tid| {
            let [nr, a1, a2, a3, a4, a5, a6, a7] = call.as_args();
            {
                CALL_FOR_THREAD.with(|cft| {
                    let cft_rc = cft.borrow();
                    let mut cft_mtx = cft_rc.lock().unwrap();
                    let tid = *tid.borrow();
                    assert!(cft_mtx.get(&tid).is_none());
                    cft_mtx.insert(tid, call)
                });
            }
            let call = crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap();

            let mut xsc_borrowed = xsc.borrow_mut();
            let xsc_as_mut = xsc_borrowed.as_mut().expect(
                "not connected to server (did you forget to create a thread with xous::create_thread()?)",
            );
            loop {
                _xous_syscall_to(nr, a1, a2, a3, a4, a5, a6, a7, &call, xsc_as_mut);
                _xous_syscall_result(&mut ret, *tid.borrow(), xsc_as_mut);
                match ret {
                    Result::Error(e) => return Err(e),
                    Result::RetryCall => (),
                    other => return Ok(other),
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
    })
}

fn _xous_syscall_result(ret: &mut Result, thread_id: TID, server_connection: &ServerConnection) {
    // Check to see if this thread id has an entry in the mailbox already.
    // This will block until the hashmap is free.
    {
        let mut mailbox = server_connection.mailbox.lock().unwrap();
        if let Some(entry) = mailbox.get(&thread_id) {
            if &Result::BlockedProcess != entry {
                *ret = mailbox.remove(&thread_id).unwrap();
                return;
            }
        }
    }

    // Receive the packet back
    loop {
        // Now that we have the Stream mutex, temporarily take the Mailbox mutex to see if
        // this thread ID is there. If it is, there's no need to read via the network.
        // Note that the mailbox mutex is released if it isn't found.
        {
            let mut mailbox = server_connection.mailbox.lock().unwrap();
            if let Some(entry) = mailbox.get(&thread_id) {
                if &Result::BlockedProcess != entry {
                    *ret = mailbox.remove(&thread_id).unwrap();
                    return;
                }
            }
        }

        let mut stream = match server_connection.recv.try_lock() {
            Ok(lk) => lk,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }
            Err(e) => panic!("Receive error: {}", e),
        };

        // One more check, in case something came in while we waited for the receiver above.
        {
            let mut mailbox = server_connection.mailbox.lock().unwrap();
            if let Some(entry) = mailbox.get(&thread_id) {
                if &Result::BlockedProcess != entry {
                    *ret = mailbox.remove(&thread_id).unwrap();
                    return;
                }
            }
        }

        // This thread_id doesn't exist in the mailbox, so read additional data.
        let mut pkt = [0usize; 8];
        let mut raw_bytes = [0u8; size_of::<usize>() * 9];
        if let Err(e) = stream.read_exact(&mut raw_bytes) {
            eprintln!("Server shut down: {}", e);
            std::process::exit(0);
        }

        let mut raw_bytes_chunks = raw_bytes.chunks(size_of::<usize>());

        // Read the Thread ID, which comes across first, followed by the 8 words of
        // the message data.
        let msg_thread_id = usize::from_le_bytes(raw_bytes_chunks.next().unwrap().try_into().unwrap());
        for (pkt_word, word) in pkt.iter_mut().zip(raw_bytes_chunks) {
            *pkt_word = usize::from_le_bytes(word.try_into().unwrap());
        }

        let mut response = Result::from_args(pkt);

        // If we got a `WouldBlock`, then we need to retry the whole call
        // again. Return and retry.
        if response == Result::RetryCall {
            // If the incoming message was for this thread, return it directly.
            if msg_thread_id == thread_id {
                *ret = response;
                return;
            }

            // Otherwise, add it to the mailbox and try again.
            let mut mailbox = server_connection.mailbox.lock().unwrap();
            mailbox.insert(msg_thread_id, response);
            continue;
        }

        if response == Result::BlockedProcess {
            // println!("   Waiting again");
            continue;
        }

        // Determine if this thread will have a memory packet following it.
        let call = CALL_FOR_THREAD.with(|cft| {
            let cft_borrowed = cft.borrow();
            let mut cft_mtx = cft_borrowed.lock().unwrap();
            cft_mtx.remove(&msg_thread_id).expect("thread didn't declare whether it has data")
        });

        // If the client is passing us memory, remap the array to our own space.
        if let Result::MessageEnvelope(msg) = &mut response {
            match &mut msg.body {
                crate::Message::Move(ref mut memory_message)
                | crate::Message::Borrow(ref mut memory_message)
                | crate::Message::MutableBorrow(ref mut memory_message) => {
                    let data = vec![0u8; memory_message.buf.len()];
                    let mut data = std::mem::ManuallyDrop::new(data);
                    if let Err(e) = stream.read_exact(&mut data) {
                        eprintln!("Server shut down: {}", e);
                        std::process::exit(0);
                    }
                    data.shrink_to_fit();
                    assert_eq!(data.len(), data.capacity());
                    let len = data.len();
                    let addr = data.as_mut_ptr();
                    memory_message.buf = unsafe { crate::MemoryRange::new(addr as _, len).unwrap() };
                }
                _ => (),
            }
        }

        // If the original call contained memory, then ensure the memory we get back is correct.
        if let Some(mem) = call.memory() {
            if call.is_borrow() || call.is_mutableborrow() {
                // Read the buffer back from the remote host.
                use core::slice;
                let mut data = unsafe { slice::from_raw_parts_mut(mem.as_mut_ptr(), mem.len()) };

                // If it's a Borrow, verify the contents haven't changed.
                let previous_data = if call.is_borrow() { Some(data.to_vec()) } else { None };

                if let Err(e) = stream.read_exact(&mut data) {
                    eprintln!("Server shut down: {}", e);
                    std::process::exit(0);
                }

                // If it is an immutable borrow, verify the contents haven't changed somehow
                if let Some(previous_data) = previous_data {
                    assert_eq!(data, previous_data.as_slice());
                }
            }

            if call.is_move() {
                // In a hosted environment, the message contents are leaked when
                // it gets converted into a MemoryMessage. Now that the call is
                // complete, free the memory.
                mem::unmap_memory_post(mem).unwrap();
            }

            // If we're returning memory to the Server, then reconstitute the buffer we just passed,
            // and Drop it so it can be freed.
            if call.is_return_memory() {
                let rebuilt = unsafe { Vec::from_raw_parts(mem.as_mut_ptr(), mem.len(), mem.len()) };
                drop(rebuilt);
            }
        }

        // Now that we have the Stream mutex, temporarily take the Mailbox mutex to see if
        // this thread ID is there. If it is, there's no need to read via the network.
        // Note that the mailbox mutex is released if it isn't found.
        {
            // If the incoming message was for this thread, return it directly.
            if msg_thread_id == thread_id {
                *ret = response;
                return;
            }

            // Otherwise, add it to the mailbox and try again.
            let mut mailbox = server_connection.mailbox.lock().unwrap();
            mailbox.insert(msg_thread_id, response);
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
fn _xous_syscall_to(
    nr: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    call: &crate::SysCall,
    xsc: &mut ServerConnection,
) {
    // println!(
    //     "Making Syscall: {:?}",
    //     crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap()
    // );

    // Send the packet to the server
    let mut capacity = 9 * core::mem::size_of::<usize>();
    if let Some(mem) = call.memory() {
        capacity += mem.len();
    }

    let mut pkt = Vec::with_capacity(capacity);
    THREAD_ID.with(|tid| pkt.extend_from_slice(&tid.borrow().to_le_bytes()));
    for word in &[nr, a1, a2, a3, a4, a5, a6, a7] {
        pkt.extend_from_slice(&word.to_le_bytes());
    }

    // Also send memory, if it's present.
    if let Some(memory) = call.memory() {
        use core::slice;
        let data: &[u8] = unsafe { slice::from_raw_parts(memory.as_ptr(), memory.len()) };
        pkt.extend_from_slice(data);
    }

    let mut stream = xsc.send.lock().unwrap();
    if let Err(e) = stream.write_all(&pkt) {
        eprintln!("Server shut down: {}", e);
        std::process::exit(0);
    }
    // stream.flush().unwrap();
}
