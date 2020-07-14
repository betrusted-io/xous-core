pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

use std::cell::RefCell;
use std::env;
use std::io::Read;
use std::mem::size_of;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::spawn;
use std::thread_local;
use std::time::Duration;

use crate::arch::process::Process;
use crate::services::SystemServices;

use xous::{MemoryAddress, Result, SysCall, PID};

enum ThreadMessage {
    SysCall(PID, SysCall),
    NewConnection(TcpStream),
}

#[derive(Debug)]
enum BackchannelMessage {
    Exit,
    NewPid(PID),
}

use std::net::{SocketAddr, IpAddr, Ipv4Addr};
thread_local!(static NETWORK_LISTEN_ADDRESS: RefCell<SocketAddr> = RefCell::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)));

/// Set the network address for this particular thread.
#[cfg(test)]
pub fn set_listen_address(new_address: &SocketAddr) {
    NETWORK_LISTEN_ADDRESS.with(|nla| {
        let mut address = nla.borrow_mut();
        *address = *new_address;
    });
}

/// Each client gets its own connection and its own thread, which is handled here.
fn handle_connection(mut conn: TcpStream, pid: PID, chn: Sender<ThreadMessage>) {
    loop {
        let mut pkt = [0usize; 8];
        let mut incoming_word = [0u8; size_of::<usize>()];
        conn.set_nonblocking(true)
            .expect("couldn't enable nonblocking mode");
        for word in pkt.iter_mut() {
            loop {
                if let Err(e) = conn.read_exact(&mut incoming_word) {
                    // If the connection has gone away, send a `TerminateProcess` message to the main
                    // and then exit this thread.
                    if e.kind() != std::io::ErrorKind::WouldBlock {
                        // println!(
                        //     "KERNEL({}): Client disconnected: {}. Shutting down virtual process.",
                        //     pid, e
                        // );
                        chn.send(ThreadMessage::SysCall(pid, xous::SysCall::TerminateProcess))
                            .unwrap();
                        return;
                    }
                    continue;
                }
                break;
            }
            *word = usize::from_le_bytes(incoming_word);
        }
        let call = xous::SysCall::from_args(
            pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7],
        );
        match call {
            Err(e) => println!("KERNEL({}): Received invalid syscall: {:?}", pid, e),
            Ok(mut call) => {
                if let SysCall::SendMessage(ref _cid, ref mut envelope) = call {
                    match envelope {
                        xous::Message::MutableBorrow(msg)
                        | xous::Message::Borrow(msg)
                        | xous::Message::Move(msg) => {
                            let mut tmp_data = Vec::with_capacity(msg.buf.len());
                            tmp_data.resize(msg.buf.len(), 0);
                            conn.read_exact(&mut tmp_data)
                                .map_err(|_e| {
                                    chn.send(ThreadMessage::SysCall(
                                        pid,
                                        xous::SysCall::TerminateProcess,
                                    ))
                                    .unwrap();
                                })
                                .unwrap();
                            // Update the address pointer. This will get turned back into a
                            // usable pointer by casting it back into a &[T] on the other
                            // side. This is just a pointer to the start of data
                            // as well as the index into the data it points at. The lengths
                            // should still be equal once we reconstitute the data in the
                            // other process.
                            // ::debug_here::debug_here!();
                            let sliced_data = tmp_data.into_boxed_slice();
                            assert_eq!(
                                sliced_data.len(),
                                msg.buf.len(),
                                "deconstructed data {} != message buf length {}",
                                sliced_data.len(),
                                msg.buf.len()
                            );
                            msg.buf.addr = MemoryAddress::new(Box::into_raw(sliced_data)
                                as *mut usize
                                as usize)
                            .unwrap();
                        }
                        xous::Message::Scalar(_) => (),
                    }
                }
                // println!(
                //     "Received packet: {:08x} {} {} {} {} {} {} {}: {:?}",
                //     pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7], call
                // );
                chn.send(ThreadMessage::SysCall(pid, call))
                    .expect("couldn't make syscall");
            }
        }
    }
}

fn listen_thread(
    listen_addr: SocketAddr,
    chn: Sender<ThreadMessage>,
    backchannel: Receiver<BackchannelMessage>,
) {
    let client_addr = listen_addr.clone();
    // println!("KERNEL(1): Starting Xous server on {}...", listen_addr);
    let listener = TcpListener::bind(listen_addr).unwrap_or_else(|e| {
        panic!("Unable to create server: {}", e);
    });

    let mut clients = vec![];

    let pid1_thread = spawn(move || {
        let mut client = TcpStream::connect(client_addr).expect("couldn't connect to xous server");
        let mut buffer = [0; 32];
        // println!("KERNEL(1): Started PID1 idle thread");
        loop {
            match client.read(&mut buffer) {
                Ok(0) => return,
                Err(e) => panic!("KERNEL(1): Unable to read buffer: {}", e),
                Ok(x) => println!("KERNEL(1): Read {} bytes", x),
            }
        }
    });

    // Use `listener` in a nonblocking setup so that we can exit when doing tests
    listener
        .set_nonblocking(true)
        .expect("couldn't set TcpListener to nonblocking");
    loop {
        match listener.accept() {
            Ok((conn, _addr)) => {
                let thr_chn = chn.clone();

                // Spawn a new process. This process will start out in the "Setup()" state.
                chn.send(ThreadMessage::NewConnection(conn.try_clone().expect(
                    "couldn't make a copy of the network connection for the kernel",
                )))
                .expect("couldn't request a new PID");
                let new_pid = match backchannel
                    .recv()
                    .expect("couldn't receive message from main thread")
                {
                    BackchannelMessage::NewPid(p) => p,
                    x => panic!("unexpected backchannel message from main thread: {:?}", x),
                };
                // println!("KERNEL({}): New client connected from {}", new_pid, _addr);
                let conn_copy = conn.try_clone().expect("couldn't duplicate connection");
                let jh = spawn(move || handle_connection(conn, new_pid, thr_chn));
                clients.push((jh, conn_copy));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                match backchannel.recv_timeout(Duration::from_millis(10)) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        continue;
                    }
                    Err(e) => panic!(
                        "KERNEL: got error when trying to receive quit timeout: {:?} ({})",
                        e, e
                    ),
                    Ok(BackchannelMessage::NewPid(x)) => {
                        panic!("got unexpected message from main thread: new pid {}", x)
                    }
                    Ok(BackchannelMessage::Exit) => {
                        for (jh, conn) in clients {
                            use std::net::Shutdown;
                            conn.shutdown(Shutdown::Both)
                                .expect("couldn't shutdown client");
                            jh.join().expect("couldn't join client thread");
                        }
                        pid1_thread.join().unwrap();
                        return;
                    }
                }
            }
            Err(e) => {
                println!("error accepting connections: {}", e);
                return;
            }
        }
    }
}

/// The idle function is run when there are no directly-runnable processes
/// that kmain can activate. In a hosted environment,this is the primary
/// thread that handles network communications, and this function never returns.
pub fn idle() -> bool {
    // Start listening.
    let (sender, receiver) = channel();
    let (backchannel_sender, backchannel_receiver) = channel();

    let listen_addr = env::var("XOUS_LISTEN_ADDR").map(|s|
        s
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address"))
        .unwrap_or_else(|_| NETWORK_LISTEN_ADDRESS.with(|nla| *nla.borrow()));

    let listen_thread_handle =
        spawn(move || listen_thread(listen_addr, sender, backchannel_receiver));

    while let Ok(msg) = receiver.recv() {
        match msg {
            ThreadMessage::NewConnection(conn) => {
                // println!("KERNEL(?): Going to call ss.spawn_process()");
                let new_pid = SystemServices::with_mut(|ss| {
                    ss.spawn_process(process::ProcessInit::new(conn.try_clone().unwrap()), ())
                })
                .unwrap();
                // println!("KERNEL({}): SystemServices assigned new PID of {}", new_pid, new_pid);
                backchannel_sender
                    .send(BackchannelMessage::NewPid(new_pid))
                    .expect("couldn't send new pid to new connection");
            }
            ThreadMessage::SysCall(pid, call) => {
                // println!("KERNEL({}): Received syscall {:?}", pid, call);
                SystemServices::with_mut(|ss| ss.switch_to(pid, Some(1))).unwrap();
                // println!("KERNEL({}): Now running as the new process", pid);

                // If the call being made is to terminate the current process, we need to know
                // because we won't be able to send a response.
                let is_terminate = call == SysCall::TerminateProcess;
                let is_shutdown = call == SysCall::Shutdown;

                // For a "Shutdown" command, send the response before we issue the shutdown.
                // This is because the "process" will be "terminated" (the network socket will be closed),
                // and we won't be able to send the response after we're done.
                if is_shutdown {
                    // println!("KERNEL: Detected shutdown -- sending final \"Ok\" to the client");
                    let mut process = Process::current();
                    let mut response_vec = Vec::new();
                    for word in Result::Ok.to_args().iter_mut() {
                        response_vec.extend_from_slice(&word.to_le_bytes());
                    }
                    process.send(&response_vec).unwrap_or_else(|_e| {
                        // If we're unable to send data to the process, assume it's dead and terminate it.
                        // println!("Unable to send response to process: {:?} -- terminating", _e);
                        crate::syscall::handle(pid, SysCall::TerminateProcess).ok();
                    });
                    // println!("KERNEL: Done sending");
                }

                // Handle the syscall within the Xous kernel
                let response = crate::syscall::handle(pid, call).unwrap_or_else(Result::Error);

                // println!("KERNEL({}): Syscall response {:?}", pid, response);
                // There's a response if it wasn't a blocked process and we're not terminating.
                // Send the response back to the target.
                if response != Result::BlockedProcess && !is_terminate && !is_shutdown {
                    {
                        let mut process = Process::current();
                        let mut response_vec = Vec::new();
                        for word in response.to_args().iter_mut() {
                            response_vec.extend_from_slice(&word.to_le_bytes());
                        }
                        process.send(&response_vec).unwrap_or_else(|_e| {
                            // If we're unable to send data to the process, assume it's dead and terminate it.
                            // println!(
                            //     "KERNEL({}): Unable to send response to process: {:?} -- terminating",
                            //     pid, _e
                            // );
                            crate::syscall::handle(pid, SysCall::TerminateProcess).ok();
                        });
                    }
                    SystemServices::with_mut(|ss| ss.switch_from(pid, 1, true)).unwrap();
                }

                if is_shutdown {
                    backchannel_sender
                        .send(BackchannelMessage::Exit)
                        .expect("couldn't send shutdown signal");
                    break;
                }
            }
        }
    }

    // println!("Exiting Xous because the listen thread channel has closed. Waiting for thread to finish...");
    listen_thread_handle
        .join()
        .expect("error waiting for listen thread to return");

    // println!("Thank you for using Xous!");
    false
}
