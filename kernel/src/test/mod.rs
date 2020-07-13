use crate::kmain;
use std::thread::{spawn, JoinHandle};

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::time::Duration;
use xous::{rsyscall, SysCall};

mod shutdown;

fn start_kernel(server_spec: &str) -> JoinHandle<()> {
    xous::hosted::set_xous_address(server_spec);
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");

    // Launch the main thread
    let server_spec_server = server_spec.to_owned();
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
    main_thread
}

#[test]
fn shutdown() {
    let server_spec = "localhost:9999";

    // Start the server in another thread.
    let main_thread = start_kernel(server_spec);

    // This is now the client.
    xous::hosted::set_xous_address(&server_spec);

    // Send a raw `Shutdown` message to terminate the kernel.
    let call_result = rsyscall(SysCall::Shutdown);
    println!("Call result: {:?}", call_result);

    // Wait for the kernel to exit.
    main_thread.join().expect("couldn't join main thread");
}

#[test]
fn send_scalar_message() {
    let server_spec = "localhost:9998";
    // Start the server in another thread
    let main_thread = start_kernel(server_spec);

    xous::hosted::set_xous_address(server_spec);

    let xous_client_spec = server_spec.to_owned();
    let xous_server_spec = server_spec.to_owned();

    let (server_addr_send, server_addr_recv) = channel();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(&xous_client_spec);
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();
        let envelope = xous::receive_message(sid).expect("couldn't receive messages");
        assert_eq!(
            envelope.message,
            xous::Message::Scalar(xous::ScalarMessage {
                id: 1,
                arg1: 2,
                arg2: 3,
                arg3: 4,
                arg4: 5
            })
        );
    });

    // Spawn the client "process" and wait for the server address.
    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(&xous_server_spec);
        let sid = server_addr_recv.recv().unwrap();
        let conn = xous::connect(sid).expect("couldn't connect to server");
        xous::send_message(
            conn,
            xous::Message::Scalar(xous::ScalarMessage {
                id: 1,
                arg1: 2,
                arg2: 3,
                arg3: 4,
                arg4: 5,
            }),
        )
        .expect("couldn't send message");
    });

    // Wait for both processes to finish
    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_move_message() {
    let server_spec = "localhost:9997";

    let main_thread = start_kernel(server_spec);

    let xous_client_spec = server_spec.to_owned();
    let xous_server_spec = server_spec.to_owned();

    let (server_addr_send, server_addr_recv) = channel();

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(&xous_client_spec);
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();
        let envelope = xous::receive_message(sid).expect("couldn't receive messages");
        println!("Received message from {}", envelope.sender);
        let message = envelope.message;
        if let xous::Message::Move(m) = message {
            let buf = m.buf;
            let bt = unsafe {
                Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
            };
            let s = String::from_utf8_lossy(&bt);
            println!("Got message: {:?} -> \"{}\"", bt, s);
        } else {
            panic!("unexpected message type");
        }

        // println!("SERVER: Received message: {:?}", msg);
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(&xous_server_spec);
        // println!("CLIENT: Waiting for server address...");
        let sid = server_addr_recv.recv().unwrap();
        // println!("CLIENT: Connecting to server {:?}", sid);
        let conn = xous::connect(sid).expect("couldn't connect to server");
        let msg = xous::carton::Carton::from_bytes(format!("Hello, world!").as_bytes());
        xous::send_message(conn, xous::Message::Move(msg.into_message(0)))
            .expect("couldn't send a message");
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}
