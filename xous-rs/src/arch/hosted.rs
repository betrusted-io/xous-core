use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread_local;

use crate::{Result, PID, TID};

pub type ThreadInit = ();
pub type ProcessInit = ();

pub struct ProcessArgs<F: FnOnce()> {
    main: Option<F>,
}

impl<F> ProcessArgs<F>
where
    F: FnOnce(),
{
    pub fn new(main: F) -> ProcessArgs<F> {
        ProcessArgs { main: Some(main) }
    }
}

pub struct WaitHandle<T>(std::thread::JoinHandle<T>);
pub struct ProcessHandle(std::thread::JoinHandle<()>);

#[derive(Clone)]
struct ServerConnection {
    send: Arc<Mutex<TcpStream>>,
    recv: Arc<Mutex<TcpStream>>,
    mailbox: Arc<Mutex<HashMap<TID, Result>>>,
}

pub fn thread_to_args(call: usize, _init: &ThreadInit) -> [usize; 8] {
    [call, 0, 0, 0, 0, 0, 0, 0]
}

pub fn process_to_args(call: usize, _init: &ProcessInit) -> [usize; 8] {
    [call, 0, 0, 0, 0, 0, 0, 0]
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
    Ok(())
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
    Ok(())
}

thread_local!(static NETWORK_CONNECT_ADDRESS: RefCell<Option<SocketAddr>> = RefCell::new(None));
thread_local!(static XOUS_SERVER_CONNECTION: RefCell<Option<ServerConnection>> = RefCell::new(None));
thread_local!(static THREAD_ID: RefCell<TID> = RefCell::new(1));
thread_local!(static PROCESS_ID: RefCell<PID> = RefCell::new(PID::new(1).unwrap()));

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

/// Set the network address for this particular thread.
pub fn set_xous_address(new_address: SocketAddr) {
    NETWORK_CONNECT_ADDRESS.with(|nca| {
        let mut address = nca.borrow_mut();
        *address = Some(new_address);
        XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = None);
    });
}

/// Set the network address for this particular thread.
fn xous_address() -> SocketAddr {
    NETWORK_CONNECT_ADDRESS
        .with(|nca| *nca.borrow())
        .unwrap_or_else(default_xous_address)
}

// pub fn xous_connect() {
//     XOUS_SERVER_CONNECTION.with(|xsc| {
//         if xsc.borrow().is_none() {
//             NETWORK_CONNECT_ADDRESS.with(|nca| {
//                 let addr = nca.borrow().unwrap_or_else(default_xous_address);
//                 match xous_connect_impl(addr) {
//                     Ok(a) => *xsc.borrow_mut() = Some(a),
//                     Err(_) => panic!("couldn't connect to server"),
//                 }
//             });
//         }
//     })
// }

pub fn create_thread_pre<F, T>(_f: &F) -> core::result::Result<ThreadInit, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    Ok(())
}

pub fn create_thread_post<F, T>(
    f: F,
    thread_id: TID,
) -> core::result::Result<WaitHandle<T>, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let server_address = xous_address();
    let server_connection =
        XOUS_SERVER_CONNECTION.with(|xsc| xsc.borrow().as_ref().unwrap().clone());
    let process_id = PROCESS_ID.with(|pid| *pid.borrow());
    Ok(std::thread::Builder::new()
        .spawn(move || {
            set_xous_address(server_address);
            THREAD_ID.with(|tid| *tid.borrow_mut() = thread_id);
            PROCESS_ID.with(|pid| *pid.borrow_mut() = process_id);
            XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = Some(server_connection));
            f()
        })
        .map(WaitHandle)
        .map_err(|_| crate::Error::InternalError)?)
}

pub fn wait_thread<T>(joiner: WaitHandle<T>) -> crate::SysCallResult {
    joiner
        .0
        .join()
        .map(|_| Result::Ok)
        .map_err(|_| crate::Error::InternalError)
}

/// If no connection exists, create a new connection to the server. This means
/// our parent PID will be PID1. Otherwise, reuse the same connection.
pub fn create_process_pre<F>(
    _args: &ProcessArgs<F>,
) -> core::result::Result<ProcessInit, crate::Error>
where
    F: FnOnce(),
{
    // Ensure there is a connection, because after this function returns
    // we'll make a syscall with CreateProcess().
    XOUS_SERVER_CONNECTION.with(|xsc| {
        let mut xsc = xsc.borrow_mut();
        if xsc.is_none() {
            NETWORK_CONNECT_ADDRESS.with(|nca| {
                let addr = nca.borrow().unwrap_or_else(default_xous_address);
                match xous_connect_impl(addr) {
                    Ok(a) => *xsc = Some(a),
                    Err(_) => {
                        return Err(crate::Error::InternalError);
                    }
                }
                Ok(())
            })
        } else {
            Ok(())
        }
    })
}

pub fn create_process_post<F>(
    mut args: ProcessArgs<F>,
    pid: PID,
) -> core::result::Result<ProcessHandle, crate::Error>
where
    F: FnOnce() + Send + 'static,
{
    let server_spec = xous_address();

    Ok(std::thread::Builder::new()
        .spawn(move || {
            set_xous_address(server_spec);
            xous_connect_impl(server_spec).unwrap();
            PROCESS_ID.with(|p| *p.borrow_mut() = pid.unwrap());
            (args.main.take().unwrap())();
        })
        .map(ProcessHandle)
        .map_err(|_| crate::Error::InternalError)?)
}

pub fn wait_process(joiner: ProcessHandle) -> crate::SysCallResult {
    joiner
        .0
        .join()
        .map(|_| Result::Ok)
        .map_err(|_| crate::Error::InternalError)
}

fn xous_connect_impl(addr: SocketAddr) -> core::result::Result<ServerConnection, ()> {
    // println!("Opening connection to Xous server @ {}...", addr);
    match TcpStream::connect(addr) {
        Ok(mut conn) => {
            let mut pid = [0u8; 1];
            // conn.read_exact(&mut pid).unwrap();
            Ok(ServerConnection {
                send: Arc::new(Mutex::new(conn.try_clone().unwrap())),
                recv: Arc::new(Mutex::new(conn)),
                mailbox: Arc::new(Mutex::new(HashMap::new())),
            })
        }
        Err(e) => {
            eprintln!("Unable to connect to Xous server: {}", e);
            eprintln!(
                "Ensure Xous is running, or specify this process as an argument to the kernel"
            );
            Err(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub fn _xous_syscall(
    nr: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    ret: &mut Result,
) {
    XOUS_SERVER_CONNECTION.with(|xsc| {
        THREAD_ID.with(|tid| {
            let call = crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap();

            if xsc.borrow().is_none() {
                panic!("Not connected to server!");
            }
            _xous_syscall_to(
                nr,
                a1,
                a2,
                a3,
                a4,
                a5,
                a6,
                a7,
                &call,
                &mut xsc.borrow_mut().as_mut().unwrap().send.lock().unwrap(),
            );
            _xous_syscall_result(
                &call,
                ret,
                *tid.borrow(),
                xsc.borrow_mut().as_mut().unwrap(),
            );
        })
    });
}

fn _xous_syscall_result(
    call: &crate::SysCall,
    ret: &mut Result,
    thread_id: TID,
    server_connection: &ServerConnection,
) {
    // Check to see if this thread id has an entry in the mailbox already.
    // This will block until the hashmap is free.
    {
        let mut mailbox = server_connection.mailbox.lock().unwrap();
        if let Some(entry) = mailbox.remove(&thread_id) {
            if Result::BlockedProcess != entry {
                *ret = entry;
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
            if let Some(entry) = mailbox.remove(&thread_id) {
                if Result::BlockedProcess != entry {
                    *ret = entry;
                    return;
                }
            }
        }

        let mut stream = match server_connection.recv.try_lock() {
            Ok(lk) => lk,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            Err(e) => panic!("Receive error: {}", e),
        };

        // This thread_id doesn't exist in the mailbox, so read additional data.
        let mut pkt = [0usize; 8];
        let mut raw_bytes = [0u8; size_of::<usize>() * 9];
        stream.read_exact(&mut raw_bytes).expect("Server shut down");
        use std::convert::TryInto;

        let mut raw_bytes_chunks = raw_bytes.chunks(size_of::<usize>());

        // Read the Thread ID, which comes across first, followed by the 8 words of
        // the message data.
        let msg_thread_id =
            usize::from_le_bytes(raw_bytes_chunks.next().unwrap().try_into().unwrap());
        for (pkt_word, word) in pkt.iter_mut().zip(raw_bytes_chunks) {
            *pkt_word = usize::from_le_bytes(word.try_into().unwrap());
        }

        let response = Result::from_args(pkt);

        // println!("   Response: {:?}", response);
        if Result::BlockedProcess == response {
            // println!("   Waiting again");
            continue;
        }
        if let crate::SysCall::SendMessage(_, ref msg) = call {
            match msg {
                crate::Message::MutableBorrow(crate::MemoryMessage {
                    id: _id,
                    buf,
                    offset: _offset,
                    valid: _valid,
                }) => {
                    // Read the buffer back from the remote host.
                    use core::slice;
                    let mut data =
                        unsafe { slice::from_raw_parts_mut(buf.addr.get() as _, buf.size.get()) };
                    stream.read_exact(&mut data).expect("Server shut down");
                    // pkt.extend_from_slice(data);
                }

                crate::Message::Borrow(crate::MemoryMessage {
                    id: _id,
                    buf,
                    offset: _offset,
                    valid: _valid,
                }) => {
                    // Read the buffer back from the remote host and ensure it's the same
                    use core::slice;
                    let mut check_data = Vec::new();
                    check_data.resize(buf.len(), 0);
                    let data = unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()) };
                    stream
                        .read_exact(&mut check_data)
                        .expect("Server shut down");

                    assert_eq!(data, check_data.as_slice());
                }

                crate::Message::Move(crate::MemoryMessage {
                    id: _id,
                    buf: _buf,
                    offset: _offset,
                    valid: _valid,
                }) => (),
                // Nothing to do for Immutable borrow, since the memory can't change
                crate::Message::Scalar(_) => (),
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
    xsc: &mut TcpStream,
) {
    // println!(
    //     "Making Syscall: {:?}",
    //     crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap()
    // );

    // Send the packet to the server
    let mut pkt = vec![];
    THREAD_ID.with(|tid| pkt.extend_from_slice(&tid.borrow().to_le_bytes()));
    for word in &[nr, a1, a2, a3, a4, a5, a6, a7] {
        pkt.extend_from_slice(&word.to_le_bytes());
    }
    if let crate::SysCall::SendMessage(_, ref msg) = call {
        match msg {
            crate::Message::MutableBorrow(crate::MemoryMessage {
                id: _id,
                buf,
                offset: _offset,
                valid: _valid,
            })
            | crate::Message::Borrow(crate::MemoryMessage {
                id: _id,
                buf,
                offset: _offset,
                valid: _valid,
            })
            | crate::Message::Move(crate::MemoryMessage {
                id: _id,
                buf,
                offset: _offset,
                valid: _valid,
            }) => {
                use core::slice;
                let data: &[u8] =
                    unsafe { slice::from_raw_parts(buf.addr.get() as _, buf.size.get()) };
                pkt.extend_from_slice(data);
            }
            crate::Message::Scalar(_) => (),
        }
    }

    xsc.write_all(&pkt).expect("Server shut down");
}
