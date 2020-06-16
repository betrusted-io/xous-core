pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

use std::env;
use std::io::{Read, Write};
use std::mem::size_of;
use std::net::{TcpListener, TcpStream};
use std::process::exit;
use std::sync::mpsc::{channel, Sender};
use std::thread::{spawn};
use xous::{Result, SysCall, PID};

use crate::services::SystemServicesHandle;

fn handle_connection(mut conn: TcpStream, pid: PID, chn: Sender<(TcpStream, PID, SysCall)>) {
    loop {
        let mut pkt = [0usize; 8];
        let mut incoming_word = [0u8; size_of::<usize>()];
        for word in pkt.iter_mut() {
            conn.read_exact(&mut incoming_word).expect("Disconnection");
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
                chn.send((
                    conn.try_clone().expect("couldn't clone connection"),
                    pid,
                    call,
                )).expect("couldn't make syscall");
            }
        }
    }
}

fn listen_thread(chn: Sender<(TcpStream, PID, SysCall)>) {
    let listen_addr = env::var("XOUS_LISTEN_ADDR").unwrap_or_else(|_| "localhost:9687".to_owned());
    println!("Starting Xous server on {}...", listen_addr);
    let listener = TcpListener::bind(listen_addr).unwrap_or_else(|e| {
        eprintln!("Unable to create server: {}", e);
        exit(1);
    });
    loop {
        let (conn, addr) = listener.accept().expect("Unable to accept connection");
        println!("New client connected from {}", addr);
        let thr_chn = chn.clone();

        let new_pid = {
            let mut ss = SystemServicesHandle::get();
            ss.spawn_process(process::ProcessInit::new(conn.try_clone().unwrap()), ()).unwrap()
        };
        println!("Assigned PID {}", new_pid);
        spawn(move || handle_connection(conn, new_pid, thr_chn));
    }
}

/// The idle function is run when there are no directly-runnable processes
/// that kmain can activate. In a hosted environment,this is the primary
/// thread that handles network communications, and this function never returns.
pub fn idle() {
    // Start listening.
    let (sender, receiver) = channel();
    let listen_thread_handle = spawn(move || listen_thread(sender));

    while let Ok((mut conn, pid, call)) = receiver.recv() {
        {
            let mut ss = SystemServicesHandle::get();
            ss.switch_to(pid, Some(1)).unwrap();
        }
        let response = crate::syscall::handle(pid, call).unwrap_or_else(Result::Error);
        if response != Result::BlockedProcess {
            let mut ss = SystemServicesHandle::get();
            ss.switch_from(pid, 1, true).unwrap();
        }
        for word in response.to_args().iter_mut() {
            conn.write_all(&word.to_le_bytes()).expect("Disconnection");
        }
    }

    eprintln!("Exiting Xous because the listen thread channel has closed. Waiting for thread to finish...");
    listen_thread_handle.join().expect("error waiting for listen thread to return");

    eprintln!("Thank you for using Xous!");
    exit(0);
}
