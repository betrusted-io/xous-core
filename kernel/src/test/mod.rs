use crate::kmain;
use std::thread::spawn;

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use xous::{SysCall, rsyscall};
use std::sync::mpsc::channel;

mod shutdown;

#[test]
fn shutdown() {
    let server_spec = "localhost:9999";
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");

    // Launch the main thread
    let server_spec_server = server_spec.clone();
    let main_thread = spawn(move || {
        crate::arch::set_listen_address(&server_spec_server);
        kmain()
    });

    // This is now the client.
    xous::hosted::set_xous_address(&server_spec);

    // Connect to server
    for i in 1..11 {
        println!("Retrying connection {}/10", i);
        let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
        if res.is_ok() {
            let call_result = rsyscall(SysCall::Shutdown);
            println!("Call result: {:?}", call_result);
            break;
        }
    }

    main_thread.join().expect("couldn't join main thread");
}

#[test]
fn send_scalar_message() {
    let server_spec = "localhost:9998";
    xous::hosted::set_xous_address(server_spec);
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");

    // Launch the main thread
    let server_spec_server = server_spec.clone();
    let main_thread = spawn(move || {
        crate::arch::set_listen_address(&server_spec_server);
        kmain()
    });

    // Connect to server. This first instance needs to make sure the kernel is listening.
    // let mut server_conn = None;
    let mut connected = false;
    for i in 1..11 {
        println!("Retrying connection {}/10", i);
        let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
        if res.is_ok() {
            connected = true;
            break;
        }
    }
    // Convert the Option<conn> into conn
    assert!(connected, "unable to connect to server");

    let xous_client_spec = server_spec.to_owned();
    let xous_server_spec = server_spec.to_owned();

    let (server_addr_send, server_addr_recv) = channel();

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(&xous_client_spec);
        println!("SERVER: Creating server");
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        println!("SERVER: Now listening on {:?}", sid);
        server_addr_send.send(sid).unwrap();
        println!("SERVER: Listening for message...");
        let msg = xous::receive_message(sid).expect("couldn't receive messages");
        println!("SERVER: Received message: {:?}", msg);
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(&xous_server_spec);
        println!("CLIENT: Waiting for server address...");
        let sid = server_addr_recv.recv().unwrap();
        println!("CLIENT: Connecting to server {:?}", sid);
        let conn = xous::connect(sid).expect("couldn't connect to server");
        xous::send_message(conn, xous::Message::Scalar(xous::ScalarMessage { id: 1, arg1: 2, arg2: 3, arg3: 4, arg4: 5} )).expect("couldn't send message");
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}
