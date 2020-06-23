pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

use std::env;
use std::io::Read;
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread::spawn;
use std::time::Duration;

use xous::{Result, SysCall, PID};

use crate::arch::process::ProcessHandle;
use crate::services::SystemServicesHandle;

/// Each client gets its own connection and its own thread, which is handled here.
fn handle_connection(mut conn: TcpStream, pid: PID, chn: Sender<(PID, SysCall)>) {
    loop {
        let mut pkt = [0usize; 8];
        let mut incoming_word = [0u8; size_of::<usize>()];
        for word in pkt.iter_mut() {
            if let Err(e) = conn.read_exact(&mut incoming_word) {
                println!(
                    "Client {} disconnected: {}. Shutting down virtual process.",
                    pid, e
                );
                let call = xous::SysCall::TerminateProcess;
                chn.send((pid, call)).unwrap();
                return;
            }
            *word = usize::from_le_bytes(incoming_word);
        }
        let call = xous::SysCall::from_args(
            pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7],
        );
        match call {
            Err(e) => println!("Received invalid syscall: {:?}", e),
            Ok(call) => {
                println!(
                    "Received packet: {:08x} {} {} {} {} {} {} {}: {:?}",
                    pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7], call
                );
                chn.send((pid, call)).expect("couldn't make syscall");
            }
        }
    }
}

fn listen_thread(chn: Sender<(PID, SysCall)>, quit: Receiver<()>) {
    let listen_addr = env::var("XOUS_LISTEN_ADDR").unwrap_or_else(|_| "localhost:9687".to_owned());
    println!("Starting Xous server on {}...", listen_addr);
    let listener = TcpListener::bind(listen_addr).unwrap_or_else(|e| {
        panic!("Unable to create server: {}", e);
    });

    // Use `listener` in a nonblocking setup so that we can exit when doing tests
    listener.set_nonblocking(true).expect("couldn't set TcpListener to nonblocking");
    loop {
        match listener.accept() {
            Ok((conn, addr)) => {
                println!("New client connected from {}", addr);
                let thr_chn = chn.clone();

                let new_pid = {
                    let mut ss = SystemServicesHandle::get();
                    ss.spawn_process(process::ProcessInit::new(conn.try_clone().unwrap()), ())
                        .unwrap()
                };
                println!("Assigned PID {}", new_pid);
                spawn(move || handle_connection(conn, new_pid, thr_chn));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                match quit.recv_timeout(Duration::from_millis(10)) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        continue;
                    },
                    _ => {
                        return;
                    }
                }
            },
            Err(e) => {
                eprintln!("error accepting connections: {}", e);
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
    let (term_sender, term_receiver) = channel();
    let listen_thread_handle = spawn(move || listen_thread(sender, term_receiver));

    // // Spawn initial programs
    // let all_args = env::args();
    // all_args.next();
    // for i in all_args {
    //     let listen_addr = env::var("XOUS_LISTEN_ADDR").unwrap_or_else(|_| "localhost:9687".to_owned();
    //     let cmd = Command::new(i).env("XOUS_LISTEN_ADDR", listen_addr)
    // }

    while let Ok((pid, call)) = receiver.recv() {
        {
            let mut ss = SystemServicesHandle::get();
            ss.switch_to(pid, Some(1)).unwrap();
        }

        // If the call being made is to terminate the current process, we need to know
        // because we won't be able to send a response.
        let is_terminate = call == SysCall::TerminateProcess;
        if call == SysCall::Shutdown {
            term_sender.send(()).expect("unable to send shutdown message");
        }

        // Handle the syscall within the Xous kernel
        let response = crate::syscall::handle(pid, call).unwrap_or_else(Result::Error);

        // There's a response if it wasn't a blocked process and we're not terminating.
        // Send the response back to the target.
        if response != Result::BlockedProcess && !is_terminate {
            {
                let mut processes = ProcessHandle::get();
                let mut response_vec = Vec::new();
                for word in response.to_args().iter_mut() {
                    response_vec.extend_from_slice(&word.to_le_bytes());
                }
                processes.send(&response_vec).unwrap_or_else(|e| {
                    // If we're unable to send data to the process, assume it's dead and terminate it.
                    println!("Unable to send response to process: {:?} -- terminating", e);
                    crate::syscall::handle(pid, SysCall::TerminateProcess).ok();
                });
            }
            let mut ss = SystemServicesHandle::get();
            ss.switch_from(pid, 1, true).unwrap();
        }
    }

    eprintln!("Exiting Xous because the listen thread channel has closed. Waiting for thread to finish...");
    listen_thread_handle
        .join()
        .expect("error waiting for listen thread to return");

    eprintln!("Thank you for using Xous!");
    false
}
