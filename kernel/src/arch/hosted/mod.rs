pub mod irq;
pub mod mem;
pub mod process;
pub mod syscall;

use xous::{SysCall, Error, PID};
use lazy_static::lazy_static;
use std::mem::size_of;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread::{spawn, JoinHandle};

type SCResult = core::result::Result<SysCall, Error>;

lazy_static! {
    pub static ref LISTEN_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
    pub static ref MESSAGE_RECEIVER: Mutex<Option<Receiver<SCResult>>> = Mutex::new(None);
}


pub fn current_pid() -> PID {
    unimplemented!()
}

fn handle_connection(mut conn: TcpStream, chn: Sender<SCResult>) {
    loop {
        let mut pkt = [0usize; 8];
        let mut incoming_word = [0u8; size_of::<usize>()];
        for word in pkt.iter_mut() {
            conn.read_exact(&mut incoming_word).expect("Disconnection");
            *word = usize::from_le_bytes(incoming_word);
        }
        let call = xous::SysCall::from_args(pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7]);
        println!(
            "Received packet: {:08x} {} {} {} {} {} {} {}: {:?}",
            pkt[0], pkt[1], pkt[2], pkt[3], pkt[4], pkt[5], pkt[6], pkt[7], call
        );
        chn.send(call);
    }
}

fn listen_thread(chn: Sender<SCResult>) {
    println!("Starting Xous server on localhost:9687...");
    let listener = TcpListener::bind("localhost:9687").expect("Unable to bind server");
    loop {
        let (conn, addr) = listener.accept().expect("Unable to accept connection");
        println!("New client connected from {}", addr);
        let thr_chn = chn.clone();
        spawn(move || handle_connection(conn, thr_chn));
    }
}

pub fn idle() {
    // Start listening, if we aren't already
    let listen_thread_obj = &mut *LISTEN_THREAD.lock().unwrap();
    if listen_thread_obj.is_none() {
        let (sender, receiver) = channel();
        *listen_thread_obj = Some(spawn(move || listen_thread(sender)));
        *MESSAGE_RECEIVER.lock().unwrap() = Some(receiver);
    }

    loop {
        // If the message is a RETURN_FROM_RESUME, return.
    }
}
