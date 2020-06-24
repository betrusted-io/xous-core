use crate::kmain;
use std::thread::spawn;

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use xous::{rsyscall_to, SysCall, Result};

mod shutdown;

#[test]
fn shutdown() {
    let server_spec = "localhost:9999";
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");
    let server_spec = server_spec.to_owned();

    // Launch the main thread
    let main_thread = spawn(|| kmain(Some(server_spec)));

    // Connect to server
    for i in 1..11 {
        println!("Retrying connection {}/10", i);
        let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
        if let Ok(mut stream) = res {
            let call_result = rsyscall_to(SysCall::Shutdown, &mut stream);
            println!("Call result: {:?}", call_result);
            break;
        }
    }

    main_thread.join().expect("couldn't join main thread");
}

#[test]
fn send_scalar_message() {
    let server_spec = "localhost:9998";
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");
    let server_spec = server_spec.to_owned();

    // Launch the main thread
    let main_thread = spawn(|| kmain(Some(server_spec)));

    // Connect to server. This first instance needs to make sure the kernel is listening.
    let mut server_conn = None;
    for i in 1..11 {
        println!("Retrying connection {}/10", i);
        let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
        if let Ok(stream) = res {
            server_conn = Some(stream);
            break;
        }
    }

    // Convert the Option<conn> into conn
    let mut server_conn = server_conn.expect("unable to connect to server");

    // The client instance should connect instantly.
    let mut client_conn = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200)).expect("client couldn't connect");

    let server_xous_addr = match rsyscall_to(SysCall::CreateServer(5), &mut server_conn).expect("couldn't create xous server") {
        Result::ServerID(sid) => sid,
        x => panic!("unexpected syscall result: {:?}", x),
    };

    eprintln!("CLIENT: Connecting to server {:?}", server_xous_addr);
    let client_xous_conn = match rsyscall_to(SysCall::Connect(server_xous_addr), &mut client_conn) {
        Err(e) => panic!("unable to connect to xous server: {:?}", e),
        Ok(Result::ConnectionID(i)) => i,
        Ok(o) => panic!("unexpected return value: {:?}", o),
    };

    eprintln!("CLIENT: Calling SendMessage...");
    match rsyscall_to(
        SysCall::SendMessage(client_xous_conn, xous::Message::Scalar(xous::ScalarMessage { id: 1, arg1: 2, arg2: 3, arg3: 4, arg4: 5} )),
        &mut client_conn).expect("couldn't send message") {
        Result::Ok => (),
        x => panic!("Unexpected message result: {:?}", x),
    }

    std::thread::sleep(Duration::from_secs(1));
    eprintln!("SERVER: Calling ReceiveMessage...");
    let incoming_message = match rsyscall_to(
        SysCall::ReceiveMessage(server_xous_addr),
        &mut server_conn
    ) {
        Err(e) => panic!("couldn't receive message: {:?}", e),
        Ok(Result::Message(m)) => m,
        Ok(o) => panic!("received invalid message: {:?}", o),
    };
    eprintln!("SERVER: Received message: {:?}", incoming_message);

    // Any process ought to be able to shut down the system currently.
    rsyscall_to(SysCall::Shutdown, &mut client_conn).expect("unable to shutdown server");

    eprintln!("Joining main thread, waiting for it to quit");
    main_thread.join().expect("couldn't join main thread");
}
