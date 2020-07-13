use std::cell::RefCell;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::TcpStream;
use std::thread_local;

use crate::Result;

thread_local!(static NETWORK_CONNECT_ADDRESS: RefCell<String> = RefCell::new("localhost:9687".to_owned()));
thread_local!(static XOUS_SERVER_CONNECTION: RefCell<Option<TcpStream>> = RefCell::new(None));

/// Set the network address for this particular thread.
pub fn set_xous_address(new_address: &str) {
    NETWORK_CONNECT_ADDRESS.with(|nca| {
        let mut address = nca.borrow_mut();
        *address = new_address.to_owned();
        XOUS_SERVER_CONNECTION.with(|xsc| *xsc.borrow_mut() = None);
    });
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
                println!("Opening connection to Xous server @ {}...", nca.borrow());
                let conn = TcpStream::connect(nca.borrow().as_str()).unwrap();
                *xsc.borrow_mut() = Some(conn);
            });
        }
        // let xsc: &mut TcpStream = (*xous_server_connection).as_mut().unwrap();
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

        println!("   Response: {:?}", *ret);
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
                    crate::Message::Borrow(_) => (),
                    crate::Message::Scalar(_) => (),
                }
            }
            return;
        }
    }
}
