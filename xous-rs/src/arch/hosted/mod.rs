use std::cell::RefCell;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::TcpStream;
use std::thread_local;

use crate::{Result, ThreadID};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
pub type ContextInit = ();
pub struct WaitHandle<T>(std::thread::JoinHandle<T>);

pub fn context_to_args(call: usize, _init: &ContextInit) -> [usize; 8] {
    [call, 0, 0, 0, 0, 0, 0, 0]
}

pub fn args_to_context(
    _a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    _a6: usize,
    _a7: usize,
) -> core::result::Result<ContextInit, crate::Error> {
    Ok(())
}

thread_local!(static NETWORK_CONNECT_ADDRESS: RefCell<SocketAddr> = RefCell::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0)));
thread_local!(static XOUS_SERVER_CONNECTION: RefCell<Option<TcpStream>> = RefCell::new(None));
thread_local!(static THREAD_ID: RefCell<ThreadID> = RefCell::new(1));

/// Set the network address for this particular thread.
pub fn set_xous_address(new_address: SocketAddr) {
    NETWORK_CONNECT_ADDRESS.with(|nca| {
        let mut address = nca.borrow_mut();
        *address = new_address;
        XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = None);
    });
}

/// Set the network address for this particular thread.
pub fn xous_address() -> SocketAddr {
    NETWORK_CONNECT_ADDRESS.with(|nca| *nca.borrow())
}

pub fn create_thread_pre<F, T>(_f: &F) -> core::result::Result<ContextInit, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    Ok(())
}

pub fn create_thread_post<F, T>(
    f: F,
    thread_id: ThreadID,
) -> core::result::Result<WaitHandle<T>, crate::Error>
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let server_address = xous_address();
    let server_connection = XOUS_SERVER_CONNECTION.with(|xsc| xsc.borrow().as_ref().unwrap().try_clone().unwrap());
    Ok(std::thread::Builder::new()
        .spawn(move || {
            set_xous_address(server_address);
            THREAD_ID.with(|tid| *tid.borrow_mut() = thread_id);
            XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = Some(server_connection));
            f()
        })
        .map(|j| WaitHandle(j))
        .map_err(|_| crate::Error::InternalError)?)
}

pub fn wait_thread<T>(joiner: WaitHandle<T>) -> crate::SysCallResult {
    joiner
        .0
        .join()
        .map(|_| Result::Ok)
        .map_err(|_| crate::Error::InternalError)
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
        if xsc.borrow().is_none() {
            NETWORK_CONNECT_ADDRESS.with(|nca| {
                // println!("Opening connection to Xous server @ {}...", nca.borrow());
                let conn = TcpStream::connect(*nca.borrow()).unwrap();
                *xsc.borrow_mut() = Some(conn);
            });
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
            ret,
            xsc.borrow_mut().as_mut().unwrap(),
        )
    })
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
    ret: &mut Result,
    xsc: &mut TcpStream,
) {
    // println!(
    //     "Making Syscall: {:?}",
    //     crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap()
    // );
    let call = crate::SysCall::from_args(nr, a1, a2, a3, a4, a5, a6, a7).unwrap();

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

    // Receive the packet back
    loop {
        let mut pkt = [0usize; 8];
        let mut word = [0u8; size_of::<usize>()];
        for pkt_word in pkt.iter_mut() {
            xsc.read_exact(&mut word).expect("Server shut down");
            *pkt_word = usize::from_le_bytes(word);
        }

        *ret = Result::from_args(pkt);

        // println!("   Response: {:?}", *ret);
        if Result::BlockedProcess == *ret {
            // println!("   Waiting again");
        } else {
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
                        let mut data = unsafe {
                            slice::from_raw_parts_mut(buf.addr.get() as _, buf.size.get())
                        };
                        xsc.read_exact(&mut data).expect("Server shut down");
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
                        let data = unsafe {
                            slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len())
                        };
                        xsc.read_exact(&mut check_data).expect("Server shut down");

                        assert_eq!(data, check_data.as_slice());
                        // pkt.extend_from_slice(data);
                    }

                    crate::Message::Move(crate::MemoryMessage {
                        id: _id,
                        buf: _buf,
                        offset: _offset,
                        valid: _valid,
                    }) => (),/*{
                        let offset = buf.addr.get() as *mut u8;
                        let size = buf.size.get();
                        extern crate alloc;
                        use alloc::alloc::{dealloc, Layout};
                        let layout = Layout::from_size_align(size, 4096).unwrap();
                        // Free memory that was moved
                        unsafe {
                            dealloc(offset, layout);
                        }
                    }*/
                    // Nothing to do for Immutable borrow, since the memory can't change
                    crate::Message::Scalar(_) => (),
                }
            }
            return;
        }
    }
}
