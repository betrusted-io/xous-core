use crate::kmain;
use std::thread::{spawn, JoinHandle};

use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::time::Duration;
use xous::{rsyscall, SysCall};

mod shutdown;

const SERVER_SPEC: &str = "localhost:0";

fn start_kernel(server_spec: &str) -> (JoinHandle<()>, SocketAddr) {
    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");
    // Attempt to bind. This will fail if the port is in use.
    let temp_server = TcpListener::bind(server_addr).unwrap();
    let server_addr = temp_server.local_addr().unwrap();
    drop(temp_server);

    xous::hosted::set_xous_address(server_addr);

    // Launch the main thread
    let server_spec_server = server_addr.clone();
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
    (main_thread, server_addr)
}

#[test]
fn shutdown() {
    // Start the server in another thread.
    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);

    // This is now the client.
    xous::hosted::set_xous_address(server_spec);

    // Send a raw `Shutdown` message to terminate the kernel.
    let call_result = rsyscall(SysCall::Shutdown);
    println!("Call result: {:?}", call_result);

    // Wait for the kernel to exit.
    main_thread.join().expect("couldn't join main thread");
}

#[test]
fn send_scalar_message() {
    // Start the server in another thread
    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);

    xous::hosted::set_xous_address(server_spec);

    let (server_addr_send, server_addr_recv) = channel();

    // Spawn the server "process" (which just lives in a separate thread)
    // and receive the message. Note that we need to communicate to the
    // "Client" what our server ID is. Normally this would be done via
    // an external nameserver.
    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
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
        xous::hosted::set_xous_address(server_spec);
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
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);

    let (server_addr_send, server_addr_recv) = channel();

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();
        let envelope = xous::receive_message(sid).expect("couldn't receive messages");
        // println!("Received message from {}", envelope.sender);
        let message = envelope.message;
        if let xous::Message::Move(m) = message {
            let buf = m.buf;
            let bt = unsafe {
                Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
            };
            assert_eq!(*test_bytes, *bt);
            // let s = String::from_utf8_lossy(&bt);
            // println!("Got message: {:?} -> \"{}\"", bt, s);
        } else {
            panic!("unexpected message type");
        }

        // println!("SERVER: Received message: {:?}", msg);
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
        // println!("CLIENT: Waiting for server address...");
        let sid = server_addr_recv.recv().unwrap();
        // println!("CLIENT: Connecting to server {:?}", sid);
        let conn = xous::connect(sid).expect("couldn't connect to server");
        let msg = xous::carton::Carton::from_bytes(test_bytes);
        xous::send_message(conn, xous::Message::Move(msg.into_message(0)))
            .expect("couldn't send a message");
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_borrow_message() {
    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = channel();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
        println!("SERVER: Creating server...");
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();
        println!("SERVER: Receiving message...");
        let envelope = xous::receive_message(sid).expect("couldn't receive messages");
        println!("SERVER: Received message from {}", envelope.sender);
        let message = envelope.message;
        if let xous::Message::Borrow(m) = message {
            let buf = m.buf;
            let bt = unsafe {
                Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
            };
            assert_eq!(*test_bytes, *bt);
            let s = String::from_utf8_lossy(&bt);
            println!("SERVER: Got message: {:?} -> \"{}\"", bt, s);
            xous::return_memory(envelope.sender, m.buf).unwrap();
            println!("SERVER: Returned memory");
        } else {
            panic!("unexpected message type");
        }
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(server_spec);

        // Get the server address (out of band) so we know what to connect to
        println!("CLIENT: Waiting for server to start...");
        let sid = server_addr_recv.recv().unwrap();

        // Perform a connection to the server
        println!("CLIENT: Connecting to server...");
        let conn = xous::connect(sid).expect("couldn't connect to server");

        // Convert the message into a "Carton" that can be shipped as a message
        println!("CLIENT: Creating carton...");
        let carton = xous::carton::Carton::from_bytes(test_bytes);

        // Send the message to the server
        println!("CLIENT: Lending message...");
        carton.lend(conn, 0).expect("couldn't lend message to server");

        println!("CLIENT: Done");
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_mutableborrow_message() {
    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = channel();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();
        let envelope = xous::receive_message(sid).expect("couldn't receive messages");
        // println!("Received message from {}", envelope.sender);
        let message = envelope.message;
        if let xous::Message::MutableBorrow(m) = message {
            let buf = m.buf;
            let mut bt = unsafe {
                Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
            };
            for letter in bt.iter_mut() {
                *letter += 1;
            }
            xous::return_memory(envelope.sender, m.buf).unwrap();
        } else {
            panic!("unexpected message type");
        }
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(server_spec);

        // Get the server address (out of band) so we know what to connect to
        let sid = server_addr_recv.recv().unwrap();

        // Perform a connection to the server
        let conn = xous::connect(sid).expect("couldn't connect to server");

        // Convert the message into a "Carton" that can be shipped as a message
        let mut carton = xous::carton::Carton::from_bytes(&test_bytes);
        let mut check_bytes = test_bytes.to_vec();
        for letter in check_bytes.iter_mut() {
            *letter += 1;
        }

        // Send the message to the server
        carton.lend_mut(conn, 3).expect("couldn't mutably lend data");

        let modified_bytes: &[u8] = carton.as_ref();
        assert_eq!(&check_bytes, &modified_bytes);
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

#[test]
fn send_mutableborrow_message_long() {
    let (main_thread, server_spec) = start_kernel(SERVER_SPEC);
    let (server_addr_send, server_addr_recv) = channel();
    let test_str = "Hello, world!";
    let test_bytes = test_str.as_bytes();

    let loops = 50;

    let xous_server = spawn(move || {
        xous::hosted::set_xous_address(server_spec);
        let sid = xous::create_server(0x7884_3123).expect("couldn't create test server");
        server_addr_send.send(sid).unwrap();

        for iteration in 0..loops {
            let envelope = xous::receive_message(sid).expect("couldn't receive messages");
            let message = envelope.message;
            if let xous::Message::MutableBorrow(m) = message {
                let buf = m.buf;
                let mut bt = unsafe {
                    Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
                };
                for letter in bt.iter_mut() {
                    *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
                }
                xous::return_memory(envelope.sender, m.buf).unwrap();
            } else {
                panic!("unexpected message type");
            }
        }
    });

    let xous_client = spawn(move || {
        xous::hosted::set_xous_address(server_spec);

        // Get the server address (out of band) so we know what to connect to
        let sid = server_addr_recv.recv().unwrap();

        // Perform a connection to the server
        let conn = xous::connect(sid).expect("couldn't connect to server");

        // Convert the message into a "Carton" that can be shipped as a message
        for iteration in 0..loops {
            let mut carton = xous::carton::Carton::from_bytes(&test_bytes);
            let mut check_bytes = test_bytes.to_vec();
            for letter in check_bytes.iter_mut() {
                *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
            }

            // Send the message to the server
            carton.lend_mut(conn, 3).expect("couldn't mutably lend data");

            let modified_bytes: &[u8] = carton.as_ref();
            assert_eq!(&check_bytes, &modified_bytes);
        }
    });

    xous_server.join().expect("couldn't join server process");
    xous_client.join().expect("couldn't join client process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

// #[test]
// fn multiple_contexts() {
//     // Start the kernel in its own thread
//     let (main_thread, server_spec) = start_kernel(SERVER_SPEC);

//     let (server_addr_send, server_addr_recv) = channel();
//     xous::create_server(name)

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }
