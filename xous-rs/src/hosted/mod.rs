use lazy_static::lazy_static;

use std::io::{Read, Write};
use std::mem::size_of;
use std::net::TcpStream;
use std::sync::Mutex;

use crate::Result;

lazy_static! {
    pub static ref XOUS_SERVER_CONNECTION: Mutex<Option<TcpStream>> = Mutex::new(None);
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
    let xous_server_connection = &mut *XOUS_SERVER_CONNECTION.lock().unwrap();
    if xous_server_connection.is_none() {
        println!("Opening connection to Xous server...");
        let conn = TcpStream::connect("localhost:9687").unwrap();
        *xous_server_connection = Some(conn);
    }

    println!(
        "Syscall: {} {:08x} {} {} {} {} {} {}",
        nr, a1, a2, a3, a4, a5, a6, a7
    );

    let mut xsc = xous_server_connection.as_ref().unwrap();
    for word in &[nr, a1, a2, a3, a4, a5, a6, a7] {
        xsc.write_all(&word.to_le_bytes()).expect("Disconnection");
    }

    let mut pkt = [0usize; 8];
    let mut word = [0u8; size_of::<usize>()];
    for pkt_word in pkt.iter_mut() {
        xsc.read_exact(&mut word).expect("Disconnection");
        *pkt_word = usize::from_le_bytes(word);
    }

    *ret = Result::from_args(pkt);
}
