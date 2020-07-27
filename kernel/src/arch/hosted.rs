pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

use std::cell::RefCell;
use std::env;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::spawn;
use std::thread_local;
use std::time::Duration;

use crate::arch::process::Process;
use crate::services::SystemServices;

use xous::{MemoryAddress, ProcessInit, Result, SysCall, PID, TID};

enum ThreadMessage {
    SysCall(PID, TID, SysCall),
    NewConnection(TcpStream, [u8; 16] /* access key */),
}

#[derive(Debug)]
enum BackchannelMessage {
    Exit,
    NewPid(PID),
}

thread_local!(static NETWORK_LISTEN_ADDRESS: RefCell<SocketAddr> = RefCell::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080)));
thread_local!(static SEND_ADDR: RefCell<Option<Sender<SocketAddr>>> = RefCell::new(None));
thread_local!(static PID1_KEY: RefCell<[u8; 16]> = RefCell::new([0u8; 16]));

#[cfg(test)]
pub fn set_pid1_key(new_key: [u8; 16]) {
    PID1_KEY.with(|p1k| *p1k.borrow_mut() = new_key);
}

/// Set the network address for this particular thread.
#[cfg(test)]
pub fn set_listen_address(new_address: &SocketAddr) {
    NETWORK_LISTEN_ADDRESS.with(|nla| {
        let mut address = nla.borrow_mut();
        *address = *new_address;
    });
}

/// Set the network address for this particular thread.
#[cfg(test)]
pub fn set_send_addr(send_addr: Sender<SocketAddr>) {
    SEND_ADDR.with(|sa| {
        *sa.borrow_mut() = Some(send_addr);
    });
}

/// Each client gets its own connection and its own thread, which is handled here.
fn handle_connection(
    mut conn: TcpStream,
    pid: PID,
    chn: Sender<ThreadMessage>,
    should_exit: std::sync::Arc<core::sync::atomic::AtomicBool>,
) {
    loop {
        let mut pkt = [0usize; 9];
        let mut incoming_word = [0u8; size_of::<usize>()];
        conn.set_nonblocking(false)
            .expect("couldn't enable nonblocking mode");
        conn.set_read_timeout(Some(Duration::from_millis(1000)))
            .unwrap();
        for word in pkt.iter_mut() {
            loop {
                if should_exit.load(core::sync::atomic::Ordering::Relaxed) {
                    chn.send(ThreadMessage::SysCall(
                        pid,
                        1,
                        xous::SysCall::TerminateProcess,
                    ))
                    .unwrap();
                }
                if let Err(e) = conn.read_exact(&mut incoming_word) {
                    // If the connection has gone away, send a `TerminateProcess` message to the main
                    // and then exit this thread.
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut
                    {
                        continue;
                    } else {
                        // println!(
                        //     "KERNEL({}): Client disconnected: {} ({:?}). Shutting down virtual process.",
                        //     pid, e, e
                        // );
                        chn.send(ThreadMessage::SysCall(
                            pid,
                            1,
                            xous::SysCall::TerminateProcess,
                        ))
                        .unwrap();
                        return;
                    }
                }
                break;
            }
            *word = usize::from_le_bytes(incoming_word);
        }

        let thread_id = pkt[0];
        let call = xous::SysCall::from_args(
            pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7], pkt[8],
        );

        match call {
            Err(e) => println!("KERNEL({}): Received invalid syscall: {:?}", pid, e),
            Ok(mut call) => {
                if let SysCall::SendMessage(ref _cid, ref mut envelope) = call {
                    match envelope {
                        xous::Message::MutableBorrow(msg)
                        | xous::Message::Borrow(msg)
                        | xous::Message::Move(msg) => {
                            let mut tmp_data = vec![0; msg.buf.len()];
                            conn.read_exact(&mut tmp_data)
                                .map_err(|_e| {
                                    // println!("KERNEL({}): Read Error {}", pid, _e);
                                    chn.send(ThreadMessage::SysCall(
                                        pid,
                                        thread_id,
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
                            msg.buf.addr = match MemoryAddress::new(Box::into_raw(sliced_data)
                                as *mut usize
                                as usize)
                            {
                                Some(a) => a,
                                _ => unreachable!(),
                            };
                        }
                        xous::Message::Scalar(_) => (),
                    }
                }
                // println!(
                //     "Received packet: {:08x} {} {} {} {} {} {} {}: {:?}",
                //     pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7], call
                // );
                chn.send(ThreadMessage::SysCall(pid, thread_id, call))
                    .expect("couldn't make syscall");
            }
        }
    }
}

fn listen_thread(
    listen_addr: SocketAddr,
    chn: Sender<ThreadMessage>,
    mut local_addr_sender: Option<Sender<SocketAddr>>,
    backchannel: Receiver<BackchannelMessage>,
) {
    let should_exit = std::sync::Arc::new(core::sync::atomic::AtomicBool::new(false));

    // println!("KERNEL(1): Starting Xous server on {}...", listen_addr);
    let listener = TcpListener::bind(listen_addr).unwrap_or_else(|e| {
        panic!("Unable to create server: {}", e);
    });
    // Notify the host what our kernel address is, if a listener exists.
    if let Some(las) = local_addr_sender.take() {
        las.send(listener.local_addr().unwrap()).unwrap();
    }

    let mut clients = vec![];

    // let pid1_should_exit = should_exit.clone();
    // let pid1_thread = spawn(move || {
    //     let mut client = TcpStream::connect(client_addr).expect("couldn't connect to xous server");
    //     client.set_read_timeout(Some(Duration::from_millis(100))).expect("couldn't set read timeout duration");
    //     let mut buffer = [0; size_of::<usize>() * 9];
    //     // println!("KERNEL(1): Started PID1 idle thread");
    //     loop {
    //         if pid1_should_exit.load(core::sync::atomic::Ordering::Relaxed) {
    //             return;
    //         }
    //         match client.read_exact(&mut buffer) {
    //             Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
    //                 continue;
    //             }
    //             Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return,
    //             Err(e) => panic!("KERNEL(1): Unable to read buffer: {}", e),
    //             Ok(()) => (), //println!("KERNEL(1): Read {} bytes", x),
    //         }
    //     }
    // });

    fn accept_new_connection(
        mut conn: TcpStream,
        chn: &Sender<ThreadMessage>,
        backchannel: &Receiver<BackchannelMessage>,
        clients: &mut Vec<(std::thread::JoinHandle<()>, TcpStream)>,
        should_exit: &std::sync::Arc<core::sync::atomic::AtomicBool>,
    ) -> bool {
        let thr_chn = chn.clone();

        let mut access_key = [0u8; 16];
        conn.read_exact(&mut access_key).unwrap();

        // Spawn a new process. This process will start out in the "Setup()" state.
        chn.send(ThreadMessage::NewConnection(
            conn.try_clone()
                .expect("couldn't make a copy of the network connection for the kernel"),
            access_key,
        ))
        .expect("couldn't request a new PID");
        let new_pid = match backchannel
            .recv()
            .expect("couldn't receive message from main thread")
        {
            BackchannelMessage::NewPid(p) => p,
            BackchannelMessage::Exit => return true,
        };
        // println!("KERNEL({}): New client connected from {}", new_pid, _addr);
        let conn_copy = conn.try_clone().expect("couldn't duplicate connection");
        let should_exit = should_exit.clone();
        let jh = spawn(move || handle_connection(conn, new_pid, thr_chn, should_exit));
        clients.push((jh, conn_copy));
        false
    }

    fn exit_server(
        should_exit: std::sync::Arc<core::sync::atomic::AtomicBool>,
        clients: Vec<(std::thread::JoinHandle<()>, TcpStream)>,
    ) {
        should_exit.store(true, core::sync::atomic::Ordering::Relaxed);
        for (jh, conn) in clients {
            use std::net::Shutdown;
            conn.shutdown(Shutdown::Both)
                .expect("couldn't shutdown client");
            jh.join().expect("couldn't join client thread");
        }
    }

    // let pid1 = listener.accept().unwrap();
    // if accept_new_connection(pid1.0, &chn, &backchannel, &mut clients, &should_exit) {
    //     exit_server(should_exit, clients);
    //     return;
    // }

    // Use `listener` in a nonblocking setup so that we can exit when doing tests
    enum ClientMessage {
        NewConnection(TcpStream),
        // BackChannel(BackchannelMessage),
    };
    let (sender, receiver) = channel();
    let tcp_sender = sender.clone();
    spawn(move || loop {
        match listener.accept() {
            Ok((conn, _addr)) => {
                tcp_sender.send(ClientMessage::NewConnection(conn)).unwrap();
            }
            Err(e) => {
                println!("error accepting connections: {}", e);
                return;
            }
        }
    });
    // spawn(move || loop {
    //     match backchannel.recv() {
    //         Err(e) => panic!(
    //             "KERNEL: got error when trying to receive quit timeout: {:?} ({})",
    //             e, e
    //         ),
    //         Ok(msg) => sender.send(ClientMessage::BackChannel(msg)).unwrap(),
    //     }
    // });

    for msg in receiver {
        match msg {
            ClientMessage::NewConnection(conn) => {
                if accept_new_connection(conn, &chn, &backchannel, &mut clients, &should_exit) {
                    exit_server(should_exit, clients);
                    return;
                }
            }
            // ClientMessage::BackChannel(BackchannelMessage::NewPid(x)) => {
            //     panic!("got unexpected message from main thread: new pid {}", x)
            // }
            // ClientMessage::BackChannel(BackchannelMessage::Exit) => {
            //     exit_server(should_exit, clients);
            //     return;
            // }
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

    // Allocate PID1 with the key we were passed.
    let pid1_key = PID1_KEY.with(|p1k| p1k.borrow().clone());
    let pid1_init = ProcessInit { key: pid1_key };
    let new_pid = SystemServices::with_mut(|ss| ss.create_process(pid1_init)).unwrap();
    assert_eq!(new_pid.get(), 1);

    let listen_addr = env::var("XOUS_LISTEN_ADDR")
        .map(|s| {
            s.to_socket_addrs()
                .expect("invalid server address")
                .next()
                .expect("unable to resolve server address")
        })
        .unwrap_or_else(|_| NETWORK_LISTEN_ADDRESS.with(|nla| *nla.borrow()));

    let listen_thread_handle = SEND_ADDR.with(|sa| {
        let sa = sa.borrow_mut().take();
        spawn(move || listen_thread(listen_addr, sender, sa, backchannel_receiver))
    });

    while let Ok(msg) = receiver.recv() {
        match msg {
            ThreadMessage::NewConnection(conn, access_key) => {
                // The new process should already have a PID registered. Convert its access key
                // into a PID, and register the connection with the server.
                println!("KERNEL: Attempting to look up access key {:?}", access_key);
                let new_pid =
                    crate::arch::process::register_connection_for_key(conn, access_key).unwrap();
                println!(
                    "KERNEL: Access key {:?} mapped to PID {}",
                    access_key, new_pid
                );

                // // On the initial connection, set the parent PID to 1. In the future
                // // there may be some sort of security token check here.
                // crate::arch::process::set_current_pid(PID::new(1).unwrap());

                // // Spawn a new process inside the kernel. This will assign us a PID.
                // let new_pid = SystemServices::with_mut(|ss| {
                //     ss.create_process(process::ProcessInit::new(conn.try_clone().unwrap()), ())
                // })
                // .unwrap();

                // Inform the backchannel of the new process ID.
                backchannel_sender
                    .send(BackchannelMessage::NewPid(new_pid))
                    .expect("couldn't send new pid to new connection");

                // conn.write_all(&new_pid.get().to_le_bytes())
                //     .expect("couldn't send pid to new process");

                // Switch to this process immediately, which moves it from `Setup(_)` to `Running(0)`.
                // Note that in this system, multiple processes can be active at once. This is
                // similar to having one core for each process
                // SystemServices::with_mut(|ss| ss.switch_to_thread(new_pid, Some(1))).unwrap();
            }
            ThreadMessage::SysCall(pid, thread_id, call) => {
                // println!("KERNEL({}): Received syscall {:?}", pid, call);
                crate::arch::process::set_current_pid(pid);
                // println!("KERNEL({}): Now running as the new process", pid);

                // If the call being made is to terminate the current process, we need to know
                // because we won't be able to send a response.
                let is_terminate = call == SysCall::TerminateProcess;
                let is_shutdown = call == SysCall::Shutdown;
                let is_shutdown = call == SysCall::Shutdown;

                // For a "Shutdown" command, send the response before we issue the shutdown.
                // This is because the "process" will be "terminated" (the network socket will be closed),
                // and we won't be able to send the response after we're done.
                if is_shutdown {
                    // println!("KERNEL: Detected shutdown -- sending final \"Ok\" to the client");
                    let mut process = Process::current();
                    let mut response_vec = Vec::new();
                    response_vec.extend_from_slice(&thread_id.to_le_bytes());
                    for word in Result::Ok.to_args().iter_mut() {
                        response_vec.extend_from_slice(&word.to_le_bytes());
                    }
                    process.send(&response_vec).unwrap_or_else(|_e| {
                        // If we're unable to send data to the process, assume it's dead and terminate it.
                        // println!("Unable to send response to process: {:?} -- terminating", _e);
                        crate::syscall::handle(pid, thread_id, SysCall::TerminateProcess).ok();
                    });
                    // println!("KERNEL: Done sending");
                }

                // Handle the syscall within the Xous kernel
                let response =
                    crate::syscall::handle(pid, thread_id, call).unwrap_or_else(Result::Error);

                // println!("KERNEL({}): Syscall response {:?}", pid, response);
                // There's a response if it wasn't a blocked process and we're not terminating.
                // Send the response back to the target.
                if response != Result::BlockedProcess && !is_terminate && !is_shutdown {
                    // The syscall may change what the current process is, but we always
                    // want to send a response to the process where the request came from.
                    // For this block, switch to the original PID, send the message, then
                    // switch back.
                    let existing_pid = crate::arch::process::current_pid();
                    crate::arch::process::set_current_pid(pid);

                    let mut process = Process::current();
                    let mut response_vec = Vec::new();
                    response_vec.extend_from_slice(&thread_id.to_le_bytes());
                    for word in response.to_args().iter_mut() {
                        response_vec.extend_from_slice(&word.to_le_bytes());
                    }
                    process.send(&response_vec).unwrap_or_else(|_e| {
                        // If we're unable to send data to the process, assume it's dead and terminate it.
                        // println!(
                        //     "KERNEL({}): Unable to send response to process: {:?} -- terminating",
                        //     pid, _e
                        // );
                        crate::syscall::handle(pid, thread_id, SysCall::TerminateProcess).ok();
                    });
                    crate::arch::process::set_current_pid(existing_pid);
                    // SystemServices::with_mut(|ss| {
                    // ss.switch_from(pid, 1, true)}).unwrap();
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
