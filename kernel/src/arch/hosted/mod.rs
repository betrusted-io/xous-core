// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

pub mod irq;
pub mod mem;
pub mod process;
pub mod rand;
pub mod syscall;

use std::cell::RefCell;
use std::convert::TryInto;
use std::env;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::thread_local;

use crossbeam_channel::{unbounded, Receiver, RecvError, RecvTimeoutError, Sender};
use xous_kernel::{ProcessInit, ProcessKey, Result, SysCall, ThreadInit, PID, TID};

use crate::arch::process::Process;
use crate::services::SystemServices;

enum ThreadMessage {
    SysCall(PID, TID, SysCall),
    NewConnection(TcpStream, ProcessKey),
}

#[derive(Debug)]
enum NewPidMessage {
    NewPid(PID),
}

#[derive(Debug)]
enum ExitMessage {
    Exit,
}

thread_local!(static NETWORK_LISTEN_ADDRESS: RefCell<SocketAddr> = RefCell::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0)));
thread_local!(static SEND_ADDR: RefCell<Option<Sender<SocketAddr>>> = RefCell::new(None));
thread_local!(static PID1_KEY: RefCell<[u8; 16]> = RefCell::new([0u8; 16]));

#[cfg(test)]
pub fn set_pid1_key(new_key: [u8; 16]) { PID1_KEY.with(|p1k| *p1k.borrow_mut() = new_key); }

/// Set the network address for this particular thread.
#[cfg(test)]
pub fn set_listen_address(new_address: &SocketAddr) {
    NETWORK_LISTEN_ADDRESS.with(|nla| {
        let mut address = nla.borrow_mut();
        *address = *new_address;
    });
}

/// Set the network address for this particular thread.
#[allow(dead_code)]
pub fn set_send_addr(send_addr: Sender<SocketAddr>) {
    SEND_ADDR.with(|sa| {
        *sa.borrow_mut() = Some(send_addr);
    });
}

use core::sync::atomic::{AtomicU64, Ordering};
static LOCAL_RNG_STATE: AtomicU64 = AtomicU64::new(2);

#[cfg(not(test))]
fn generate_pid_key() -> [u8; 16] {
    use rand_chacha::rand_core::RngCore;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    let mut process_key = [0u8; 16];
    let mut rng = ChaCha8Rng::seed_from_u64(
        LOCAL_RNG_STATE.load(Ordering::SeqCst)
            + xous_kernel::TESTING_RNG_SEED.load(core::sync::atomic::Ordering::SeqCst),
    );
    for b in process_key.iter_mut() {
        *b = rng.next_u32() as u8;
    }
    LOCAL_RNG_STATE.store(rng.next_u64(), Ordering::SeqCst);
    process_key
}

#[allow(dead_code)]
pub fn current_pid() -> PID { crate::arch::process::current_pid() }

/// Each client gets its own connection and its own thread, which is handled here.
fn handle_connection(
    conn: TcpStream,
    pid: PID,
    chn: Sender<ThreadMessage>,
    should_exit: std::sync::Arc<core::sync::atomic::AtomicBool>,
) {
    fn conn_thread(mut conn: TcpStream, sender: Sender<ThreadMessage>, pid: PID) {
        loop {
            let mut raw_data = [0u8; 9 * std::mem::size_of::<usize>()];

            // Read bytes from the connection. This will fail when the connection closes,
            // so send a `Termination` message across the channel.
            if let Err(_e) = conn.read_exact(&mut raw_data) {
                #[cfg(not(test))]
                eprintln!("KERNEL({}): client disconnected: {} -- shutting down virtual process", pid, _e);
                // sender.send(ServerMessage::Exit).ok();
                return;
            }

            let mut packet_data = [0usize; 9];
            for (bytes, word) in
                raw_data.chunks_exact(std::mem::size_of::<usize>()).zip(packet_data.iter_mut())
            {
                *word = usize::from_le_bytes(bytes.try_into().unwrap());
            }
            let thread_id = packet_data[0] as TID;
            let mut call = match crate::SysCall::from_args(
                packet_data[1],
                packet_data[2],
                packet_data[3],
                packet_data[4],
                packet_data[5],
                packet_data[6],
                packet_data[7],
                packet_data[8],
            ) {
                Ok(call) => call,
                Err(e) => {
                    eprintln!("KERNEL({}): Received invalid syscall: {:?}", pid, e);
                    eprintln!(
                        "Raw packet: {:08x} {} {} {} {} {} {} {}",
                        packet_data[0],
                        packet_data[1],
                        packet_data[2],
                        packet_data[3],
                        packet_data[4],
                        packet_data[5],
                        packet_data[6],
                        packet_data[7]
                    );
                    continue;
                }
            };

            if let Some(mem) = call.memory() {
                let mut data = vec![0u8; mem.len()];
                if conn.read_exact(&mut data).is_err() {
                    return;
                }

                let sliced_data = data.into_boxed_slice();
                assert_eq!(
                    sliced_data.len(),
                    mem.len(),
                    "deconstructed data {} != message buf length {}",
                    sliced_data.len(),
                    mem.len()
                );
                unsafe {
                    call.replace_memory(
                        xous_kernel::MemoryRange::new(
                            Box::into_raw(sliced_data) as *mut u8 as usize,
                            mem.len(),
                        )
                        .unwrap(),
                    )
                };
            }

            sender.send(ThreadMessage::SysCall(pid, thread_id, call)).unwrap();
        }
    }

    let conn_sender = chn.clone();
    let conn_thread = std::thread::Builder::new()
        .name(format!("PID {}: client connection thread", pid))
        .spawn(move || {
            conn_thread(conn, conn_sender, pid);
        })
        .unwrap();

    std::thread::Builder::new()
        .name(format!("PID {}: client should_exit thread", pid))
        .spawn(move || {
            loop {
                if should_exit.load(Ordering::Relaxed) {
                    eprintln!("KERNEL: should_exit == 1");
                    // sender.send(ServerMessage::Exit).ok();
                    // WARNING: This functionality is unimplemented right now
                    return;
                }
                std::thread::park_timeout(std::time::Duration::from_millis(100));
            }
        })
        .unwrap();

    conn_thread.join().unwrap();
    #[cfg(not(test))]
    eprintln!("KERNEL({}): Finished the thread so sending TerminateProcess", pid);
    chn.send(ThreadMessage::SysCall(pid, 1, xous_kernel::SysCall::TerminateProcess(0))).unwrap();
}

fn listen_thread(
    listen_addr: SocketAddr,
    chn: Sender<ThreadMessage>,
    mut local_addr_sender: Option<Sender<SocketAddr>>,
    new_pid_channel: Receiver<NewPidMessage>,
    exit_channel: Receiver<ExitMessage>,
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

    fn accept_new_connection(
        mut conn: TcpStream,
        chn: &Sender<ThreadMessage>,
        new_pid_channel: &Receiver<NewPidMessage>,
        clients: &mut Vec<(std::thread::JoinHandle<()>, TcpStream)>,
        should_exit: &std::sync::Arc<core::sync::atomic::AtomicBool>,
    ) -> bool {
        let thr_chn = chn.clone();

        // Read the challenge access key from the client
        let mut access_key = [0u8; 16];
        conn.read_exact(&mut access_key).unwrap();
        conn.set_nodelay(true).unwrap();

        // Spawn a new process. This process will start out in the "Allocated" state.
        chn.send(ThreadMessage::NewConnection(
            conn.try_clone().expect("couldn't make a copy of the network connection for the kernel"),
            ProcessKey::new(access_key),
        ))
        .expect("couldn't request a new PID");

        // The kernel will immediately respond with a new PID.
        let NewPidMessage::NewPid(new_pid) =
            new_pid_channel.recv().expect("couldn't receive message from main thread");
        let conn_copy = conn.try_clone().expect("couldn't duplicate connection");
        let should_exit = should_exit.clone();
        let jh = std::thread::Builder::new()
            .name(format!("kernel PID {} listener", new_pid))
            .spawn(move || handle_connection(conn, new_pid, thr_chn, should_exit))
            .expect("couldn't spawn listen thread");
        clients.push((jh, conn_copy));
        false
    }

    fn exit_server(
        should_exit: std::sync::Arc<core::sync::atomic::AtomicBool>,
        clients: Vec<(std::thread::JoinHandle<()>, TcpStream)>,
    ) {
        should_exit.store(true, Ordering::Relaxed);
        for (jh, conn) in clients {
            use std::net::Shutdown;
            conn.shutdown(Shutdown::Both).ok();
            jh.join().expect("couldn't join client thread");
        }
    }

    // Use `listener` in a nonblocking setup so that we can exit when doing tests
    enum ClientMessage {
        NewConnection(TcpStream),
        Exit,
    }
    let (sender, receiver) = unbounded();
    let tcp_sender = sender.clone();
    let exit_sender = sender;

    let (shutdown_listener, shutdown_listener_receiver) = unbounded();

    // `listener.accept()` has no way to break, so we must put it in nonblocking mode
    listener.set_nonblocking(true).unwrap();

    std::thread::Builder::new()
        .name("kernel accept thread".to_owned())
        .spawn(move || {
            loop {
                match listener.accept() {
                    Ok((conn, _addr)) => {
                        conn.set_nonblocking(false).unwrap();
                        tcp_sender.send(ClientMessage::NewConnection(conn)).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        match shutdown_listener_receiver.recv_timeout(std::time::Duration::from_millis(500)) {
                            Err(RecvTimeoutError::Timeout) => continue,
                            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        // Windows generates this error -- WSACancelBlockingCall -- when a
                        // connection is shut down while `accept()` is running. This should
                        // only happen when the system is shutting down, so ignore it.
                        if cfg!(windows) {
                            if let Some(10004) = e.raw_os_error() {
                                return;
                            }
                        }
                        eprintln!("error accepting connections: {} ({:?}) ({:?})", e, e, e.kind());
                        return;
                    }
                }
            }
        })
        .unwrap();

    // Spawn a thread to listen for the `exit` command, and relay that
    // to the main thread. This prevents us from needing to poll, since
    // all messages are coalesced into a single channel.
    std::thread::Builder::new()
        .name("kernel exit listener".to_owned())
        .spawn(move || match exit_channel.recv() {
            Ok(ExitMessage::Exit) => exit_sender.send(ClientMessage::Exit).unwrap(),
            Err(RecvError) => eprintln!("error receiving exit command"),
        })
        .unwrap();

    for msg in receiver {
        match msg {
            ClientMessage::NewConnection(conn) => {
                if accept_new_connection(conn, &chn, &new_pid_channel, &mut clients, &should_exit) {
                    break;
                }
            }
            ClientMessage::Exit => break,
        }
    }
    shutdown_listener.send(()).unwrap();
    exit_server(should_exit, clients);
}

/// The idle function is run when there are no directly-runnable processes
/// that kmain can activate. In a hosted environment,this is the primary
/// thread that handles network communications, and this function never returns.
pub fn idle() -> bool {
    // Start listening.
    let (sender, message_receiver) = unbounded();
    let (new_pid_sender, new_pid_receiver) = unbounded();
    let (exit_sender, exit_receiver) = unbounded();

    // Allocate PID1 with the key we were passed.
    let pid1_key = PID1_KEY.with(|p1k| *p1k.borrow());
    let pid1_init = ProcessInit { key: ProcessKey::new(pid1_key) };
    let process_1 = SystemServices::with_mut(|ss| ss.create_process(pid1_init)).unwrap();
    assert_eq!(process_1.pid().get(), 1);
    let _tid1 = SystemServices::with_mut(|ss| ss.create_thread(process_1.pid(), ThreadInit {})).unwrap();

    let listen_addr = env::var("XOUS_LISTEN_ADDR")
        .map(|s| {
            s.to_socket_addrs()
                .expect("invalid server address")
                .next()
                .expect("unable to resolve server address")
        })
        .unwrap_or_else(|_| NETWORK_LISTEN_ADDRESS.with(|nla| *nla.borrow()));

    #[cfg(not(test))]
    let address_receiver = {
        let (sender, receiver) = unbounded();
        set_send_addr(sender);
        receiver
    };

    let listen_thread_handle = SEND_ADDR.with(|sa| {
        let sa = sa.borrow_mut().take();
        std::thread::Builder::new()
            .name("kernel network listener".to_owned())
            .spawn(move || listen_thread(listen_addr, sender, sa, new_pid_receiver, exit_receiver))
            .expect("couldn't spawn listen thread")
    });

    #[cfg(not(test))]
    {
        let address = address_receiver.recv().unwrap();
        xous_kernel::arch::set_xous_address(address);
        println!("KERNEL: Xous server listening on {}", address);
        println!("KERNEL: Starting initial processes:");
        let mut args = std::env::args();
        args.next();

        // Set the current PID to 1, which was created above. This ensures all init processes
        // are owned by PID1.
        crate::arch::process::set_current_pid(process_1.pid());

        // Go through each arg and spawn it as a new process. Failures here will
        // halt the entire system.
        println!("  PID  |  Command");
        println!("-------+------------------");
        for arg in args {
            let process_key = generate_pid_key();
            let init = xous_kernel::ProcessInit { key: ProcessKey::new(process_key) };
            let new_process = SystemServices::with_mut(|ss| ss.create_process(init)).unwrap();
            println!(" {:^5} |  {}", new_process, arg);
            let process_args = xous_kernel::ProcessArgs::new("program", arg);
            xous_kernel::arch::create_process_post(process_args, init, new_process).expect("couldn't spawn");
        }
    }

    while let Ok(msg) = message_receiver.recv() {
        match msg {
            ThreadMessage::NewConnection(conn, access_key) => {
                // The new process should already have a PID registered. Convert its access key
                // into a PID, and register the connection with the server.
                let new_pid = crate::arch::process::register_connection_for_key(conn, access_key).unwrap();
                // println!(
                //     "KERNEL: Access key {:?} mapped to PID {}",
                //     access_key, new_pid
                // );

                // Inform the backchannel of the new process ID.
                new_pid_sender
                    .send(NewPidMessage::NewPid(new_pid))
                    .expect("couldn't send new pid to new connection");

                // conn.write_all(&new_pid.get().to_le_bytes())
                //     .expect("couldn't send pid to new process");

                // Switch to this process immediately, which moves it from `Setup(_)` to `Running(0)`.
                // Note that in this system, multiple processes can be active at once. This is
                // similar to having one core for each process
                if new_pid != PID::new(1).unwrap() {
                    SystemServices::with_mut(|ss| {
                        ss.create_thread(new_pid, ThreadInit {})?;
                        ss.switch_to_thread(new_pid, None)
                    })
                    .unwrap();
                }
            }
            ThreadMessage::SysCall(pid, thread_id, call) => {
                // let measurement_start = std::time::Instant::now();
                // println!("KERNEL({}): Received syscall {:?}", pid, call);
                crate::arch::process::set_current_pid(pid);
                // println!("KERNEL({}): Now running as the new process", pid);

                // If the call being made is to terminate the current process, we need to know
                // because we won't be able to send a response.
                let is_terminate = call == SysCall::TerminateProcess(0);
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
                        println!("Unable to send response to process: {:?} -- terminating", _e);
                        crate::syscall::handle(pid, thread_id, false, SysCall::TerminateProcess(0)).ok();
                    });
                    // println!("KERNEL: Done sending");
                }

                {
                    let current_process = crate::arch::process::Process::current();
                    if current_process.thread_exists(thread_id) {
                        SystemServices::with_mut(|ss| ss.switch_to_thread(pid, Some(thread_id))).unwrap();
                        crate::arch::process::Process::current().set_tid(thread_id).unwrap();
                    }
                }

                // Handle the syscall within the Xous kernel
                let response =
                    crate::syscall::handle(pid, thread_id, false, call).unwrap_or_else(Result::Error);

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
                    let mut capacity = 9 * core::mem::size_of::<usize>();
                    if let Some(mem) = response.memory() {
                        capacity += mem.len();
                    }
                    let mut response_vec = Vec::with_capacity(capacity);

                    response_vec.extend_from_slice(&thread_id.to_le_bytes());
                    for word in response.to_args().iter_mut() {
                        response_vec.extend_from_slice(&word.to_le_bytes());
                    }
                    if let Some(mem) = response.memory() {
                        let s = unsafe { core::slice::from_raw_parts(mem.as_ptr(), mem.len()) };
                        response_vec.extend_from_slice(s);
                    }
                    process.send(&response_vec).unwrap_or_else(|_e| {
                        // If we're unable to send data to the process, assume it's dead and terminate it.
                        eprintln!(
                            "KERNEL({}): Unable to send response to process: {:?} -- terminating",
                            pid, _e
                        );
                        crate::syscall::handle(pid, thread_id, false, SysCall::TerminateProcess(0)).ok();
                    });
                    crate::arch::process::set_current_pid(existing_pid);
                    // println!(
                    //     "KERNEL [{:2}:{:2}] Syscall took {:7} usec",
                    //     pid,
                    //     thread_id,
                    //     measurement_start.elapsed().as_micros()
                    // );
                }

                if is_shutdown {
                    exit_sender.send(ExitMessage::Exit).expect("couldn't send shutdown signal");
                    break;
                }
            }
        }
    }

    // println!("Exiting Xous because the listen thread channel has closed. Waiting for thread to finish...");
    listen_thread_handle.join().expect("error waiting for listen thread to return");

    // println!("Thank you for using Xous!");
    false
}
