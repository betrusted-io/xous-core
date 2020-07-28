use crate::kmain;
use std::thread::JoinHandle;

use std::net::ToSocketAddrs;
use std::sync::mpsc::channel;
use xous::{rsyscall, SysCall};

mod shutdown;

#[cfg(feature = "report-memory")]
use stats_alloc::{Region, Stats, StatsAlloc, INSTRUMENTED_SYSTEM};
#[cfg(feature = "report-memory")]
#[global_allocator]
static GLOBAL: &StatsAlloc<std::alloc::System> = &INSTRUMENTED_SYSTEM;

const SERVER_SPEC: &str = "127.0.0.1:0";

fn start_kernel(server_spec: &str) -> JoinHandle<()> {
    assert!(
        std::env::var("XOUS_LISTEN_ADDR").is_err(),
        "XOUS_LISTEN_ADDR environment variable must be unset to run tests"
    );
    assert!(
        std::env::var("XOUS_SERVER").is_err(),
        "XOUS_SERVER environment variable must be unset to run tests"
    );

    use rand::{thread_rng, Rng};
    let mut pid1_key = [0u8; 16];
    let mut rng = thread_rng();
    for b in pid1_key.iter_mut() {
        *b = rng.gen();
    }
    xous::arch::set_process_key(&pid1_key);

    let server_addr = server_spec
        .to_socket_addrs()
        .expect("invalid server address")
        .next()
        .expect("unable to resolve server address");
    // Attempt to bind. This will fail if the port is in use.
    // let temp_server = TcpListener::bind(server_addr).unwrap();
    // let server_addr = temp_server.local_addr().unwrap();
    // drop(temp_server);

    let (send_addr, recv_addr) = channel();

    // Launch the main thread. We pass a `send_addr` channel so that the
    // server can notify us when it's ready to listen.
    let main_thread = std::thread::spawn(move || {
        let server_spec_server = server_addr;
        crate::arch::set_pid1_key(pid1_key);
        crate::arch::set_send_addr(send_addr);
        crate::arch::set_listen_address(&server_spec_server);
        kmain()
    });
    let server_addr = recv_addr.recv().unwrap();
    println!("Got server address: {:?}", server_addr);
    xous::arch::set_xous_address(server_addr);

    // Connect to server. This first instance needs to make sure the kernel is listening.
    // let mut server_conn = None;
    // let mut connected = false;
    // for i in 1..11 {
    //     let res = TcpStream::connect_timeout(&server_addr, Duration::from_millis(200));
    //     if res.is_ok() {
    //         connected = true;
    //         break;
    //     }
    //     println!("Retrying connection {}/10", i);
    // }
    // // Convert the Option<conn> into conn
    // assert!(connected, "unable to connect to server");
    main_thread
}

// /// Spawn a new "process" with the given server spec inside the given closure
// /// and return a join handle
// fn as_process<F, R>(f: F) -> JoinHandle<R>
// where
//     F: FnOnce() -> R,
//     F: Send + 'static,
//     R: Send + 'static,
// {
//     let server_spec = xous::arch::xous_address();
//     std::thread::spawn(move || {
//         xous::arch::set_xous_address(server_spec);
//         xous::arch::xous_connect();
//         f()
//     })
// }

#[test]
fn shutdown() {
    // Start the server in another thread.
    let main_thread = start_kernel(SERVER_SPEC);

    // Send a raw `Shutdown` message to terminate the kernel.
    xous::create_process(xous::ProcessArgs::new(|| {
        rsyscall(SysCall::Shutdown).unwrap();
    }))
    .unwrap();

    // Wait for the kernel to exit.
    main_thread.join().expect("couldn't join main thread");
}

// #[test]
// fn send_scalar_message() {
//     // Start the server in another thread
//     let main_thread = start_kernel(SERVER_SPEC);

//     let (server_addr_send, server_addr_recv) = channel();

//     // Spawn the server "process" (which just lives in a separate thread)
//     // and receive the message. Note that we need to communicate to the
//     // "Client" what our server ID is. Normally this would be done via
//     // an external nameserver.
//     let xous_server = as_process(move || {
//         let sid = xous::create_server(b"send_scalar_mesg").expect("couldn't create test server");
//         server_addr_send.send(sid).unwrap();
//         let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//         assert_eq!(
//             envelope.message,
//             xous::Message::Scalar(xous::ScalarMessage {
//                 id: 1,
//                 arg1: 2,
//                 arg2: 3,
//                 arg3: 4,
//                 arg4: 5
//             })
//         );
//     });

//     // Spawn the client "process" and wait for the server address.
//     let xous_client = as_process(move || {
//         let sid = server_addr_recv.recv().unwrap();
//         let conn = xous::connect(sid).expect("couldn't connect to server");
//         xous::send_message(
//             conn,
//             xous::Message::Scalar(xous::ScalarMessage {
//                 id: 1,
//                 arg1: 2,
//                 arg2: 3,
//                 arg3: 4,
//                 arg4: 5,
//             }),
//         )
//         .expect("couldn't send message");
//     });

//     // Wait for both processes to finish
//     xous_server.join().expect("couldn't join server process");
//     xous_client.join().expect("couldn't join client process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[test]
// fn send_move_message() {
//     let test_str = "Hello, world!";
//     let test_bytes = test_str.as_bytes();

//     let main_thread = start_kernel(SERVER_SPEC);

//     let (server_addr_send, server_addr_recv) = channel();

//     let xous_server = as_process(move || {
//         let sid = xous::create_server(b"send_move_messag").expect("couldn't create test server");
//         server_addr_send.send(sid).unwrap();
//         let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//         // println!("Received message from {}", envelope.sender);
//         let message = envelope.message;
//         if let xous::Message::Move(m) = message {
//             let buf = m.buf;
//             let bt = unsafe {
//                 Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
//             };
//             assert_eq!(*test_bytes, *bt);
//         // let s = String::from_utf8_lossy(&bt);
//         // println!("Got message: {:?} -> \"{}\"", bt, s);
//         } else {
//             panic!("unexpected message type");
//         }

//         // println!("SERVER: Received message: {:?}", msg);
//     });

//     let xous_client = as_process(move || {
//         // println!("CLIENT: Waiting for server address...");
//         let sid = server_addr_recv.recv().unwrap();
//         // println!("CLIENT: Connecting to server {:?}", sid);
//         let conn = xous::connect(sid).expect("couldn't connect to server");
//         let msg = xous::carton::Carton::from_bytes(test_bytes);
//         xous::send_message(conn, xous::Message::Move(msg.into_message(0)))
//             .expect("couldn't send a message");
//     });

//     xous_server.join().expect("couldn't join server process");
//     xous_client.join().expect("couldn't join client process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[test]
// fn send_borrow_message() {
//     let main_thread = start_kernel(SERVER_SPEC);
//     let (server_addr_send, server_addr_recv) = channel();
//     let test_str = "Hello, world!";
//     let test_bytes = test_str.as_bytes();

//     let xous_server = as_process(move || {
//         // println!("SERVER: Creating server...");
//         let sid = xous::create_server(b"send_borrow_mesg").expect("couldn't create test server");
//         server_addr_send.send(sid).unwrap();
//         // println!("SERVER: Receiving message...");
//         let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//         // println!("SERVER: Received message from {}", envelope.sender);
//         let message = envelope.message;
//         if let xous::Message::Borrow(m) = message {
//             let buf = m.buf;
//             let bt = unsafe {
//                 Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
//             };
//             assert_eq!(*test_bytes, *bt);
//             // let s = String::from_utf8_lossy(&bt);
//             // println!("SERVER: Got message: {:?} -> \"{}\"", bt, s);
//             xous::return_memory(envelope.sender, m.buf).unwrap();
//         // println!("SERVER: Returned memory");
//         } else {
//             panic!("unexpected message type");
//         }
//     });

//     let xous_client = as_process(move || {
//         // Get the server address (out of band) so we know what to connect to
//         // println!("CLIENT: Waiting for server to start...");
//         let sid = server_addr_recv.recv().unwrap();

//         // Perform a connection to the server
//         // println!("CLIENT: Connecting to server...");
//         let conn = xous::connect(sid).expect("couldn't connect to server");

//         // Convert the message into a "Carton" that can be shipped as a message
//         // println!("CLIENT: Creating carton...");
//         let carton = xous::carton::Carton::from_bytes(test_bytes);

//         // Send the message to the server
//         // println!("CLIENT: Lending message...");
//         carton
//             .lend(conn, 0)
//             .expect("couldn't lend message to server");

//         // println!("CLIENT: Done");
//     });

//     xous_server.join().expect("couldn't join server process");
//     xous_client.join().expect("couldn't join client process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[test]
// fn send_mutableborrow_message() {
//     let main_thread = start_kernel(SERVER_SPEC);
//     let (server_addr_send, server_addr_recv) = channel();
//     let test_str = "Hello, world!";
//     let test_bytes = test_str.as_bytes();

//     let xous_server = as_process(move || {
//         let sid = xous::create_server(b"send_mutborrow_m").expect("couldn't create test server");
//         server_addr_send.send(sid).unwrap();
//         let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//         // println!("Received message from {}", envelope.sender);
//         let message = envelope.message;
//         if let xous::Message::MutableBorrow(m) = message {
//             let buf = m.buf;
//             let mut bt = unsafe {
//                 Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
//             };
//             for letter in bt.iter_mut() {
//                 *letter += 1;
//             }
//             xous::return_memory(envelope.sender, m.buf).unwrap();
//         } else {
//             panic!("unexpected message type");
//         }
//     });

//     let xous_client = as_process(move || {
//         // Get the server address (out of band) so we know what to connect to
//         let sid = server_addr_recv.recv().unwrap();

//         // Perform a connection to the server
//         let conn = xous::connect(sid).expect("couldn't connect to server");

//         // Convert the message into a "Carton" that can be shipped as a message
//         let mut carton = xous::carton::Carton::from_bytes(&test_bytes);
//         let mut check_bytes = test_bytes.to_vec();
//         for letter in check_bytes.iter_mut() {
//             *letter += 1;
//         }

//         // Send the message to the server
//         carton
//             .lend_mut(conn, 3)
//             .expect("couldn't mutably lend data");

//         let modified_bytes: &[u8] = carton.as_ref();
//         assert_eq!(&check_bytes, &modified_bytes);
//     });

//     xous_server.join().expect("couldn't join server process");
//     xous_client.join().expect("couldn't join client process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[test]
// fn send_mutableborrow_message_repeat() {
//     let main_thread = start_kernel(SERVER_SPEC);
//     let (server_addr_send, server_addr_recv) = channel();
//     let test_str = "Hello, world!";
//     let test_bytes = test_str.as_bytes();

//     let loops = 50;

//     let xous_server = as_process(move || {
//         let sid = xous::create_server(b"send_mutborrow_r").expect("couldn't create test server");
//         server_addr_send.send(sid).unwrap();

//         for iteration in 0..loops {
//             let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//             let message = envelope.message;
//             if let xous::Message::MutableBorrow(m) = message {
//                 let buf = m.buf;
//                 let mut bt = unsafe {
//                     Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
//                 };
//                 for letter in bt.iter_mut() {
//                     *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
//                 }
//                 xous::return_memory(envelope.sender, m.buf).unwrap();
//             } else {
//                 panic!("unexpected message type");
//             }
//         }
//     });

//     let xous_client = as_process(move || {
//         // Get the server address (out of band) so we know what to connect to
//         let sid = server_addr_recv.recv().unwrap();

//         // Perform a connection to the server
//         let conn = xous::connect(sid).expect("couldn't connect to server");

//         // Convert the message into a "Carton" that can be shipped as a message
//         for iteration in 0..loops {
//             let mut carton = xous::carton::Carton::from_bytes(&test_bytes);
//             let mut check_bytes = test_bytes.to_vec();
//             for letter in check_bytes.iter_mut() {
//                 *letter = (*letter).wrapping_add((iteration & 0xff) as u8);
//             }

//             // Send the message to the server
//             carton
//                 .lend_mut(conn, 3)
//                 .expect("couldn't mutably lend data");

//             let modified_bytes: &[u8] = carton.as_ref();
//             assert_eq!(&check_bytes, &modified_bytes);
//         }
//     });

//     xous_server.join().expect("couldn't join server process");
//     xous_client.join().expect("couldn't join client process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[cfg(feature = "report-memory")]
// #[test]
// fn measure_memory_usage() {
//     let mut reg = Region::new(&GLOBAL);
//     reg.reset();

//     {
//         // Run the "shutdown" test in its own thread. This ensures that
//         // any TLS is freed when the thread exits.
//         let jh = as_process(shutdown);
//         jh.join()
//             .expect("couldn't run shutdown test for measuring memory");
//     }

//     fn memory_in_use(start: &Stats) -> usize {
//         if start.bytes_deallocated > start.bytes_allocated {
//             eprintln!("Allocated a negative number of bytes!");
//             0
//         } else {
//             start.bytes_allocated - start.bytes_deallocated
//         }
//     }

//     let after_join = reg.change();
//     let miu = memory_in_use(&after_join);
//     println!("After test: {:#?} ({} bytes in use)", after_join, miu);
// }

/// Test that a server can be its own client
#[test]
fn server_client_same_process() {
    // Start the kernel in its own thread
    let main_thread = start_kernel(SERVER_SPEC);

    let internal_server = xous::create_process(xous::arch::ProcessArgs::new(|| {
        let server = xous::create_server(b"s_c_same_process").expect("couldn't create server");
        let connection = xous::connect(server).expect("couldn't connect to our own server");
        let msg_contents = xous::ScalarMessage {
            id: 1,
            arg1: 2,
            arg2: 3,
            arg3: 4,
            arg4: 5,
        };

        xous::send_message(connection, xous::Message::Scalar(msg_contents))
            .expect("couldn't send message");

        let msg = xous::receive_message(server).expect("couldn't receive message");

        assert_eq!(msg.message, xous::Message::Scalar(msg_contents));
    }))
    .expect("couldn't start server");

    xous::wait_process(internal_server).expect("couldn't join internal_server process");

    // Any process ought to be able to shut down the system currently.
    rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

    main_thread.join().expect("couldn't join kernel process");
}

// /// Test that one process can have multiple contexts
// #[test]
// fn multiple_contexts() {
//     // ::debug_here::debug_here!();
//     // Start the kernel in its own thread
//     let main_thread = start_kernel(SERVER_SPEC);

//     let internal_server = as_process(|| {
//         let server = xous::create_server(b"multiple_context").expect("couldn't create server");
//         let connection = xous::connect(server).expect("couldn't connect to our own server");
//         let msg_contents = xous::ScalarMessage {
//             id: 1,
//             arg1: 2,
//             arg2: 3,
//             arg3: 4,
//             arg4: 5,
//         };

//         let mut server_threads = vec![];
//         for _ in 0..30 {
//             server_threads.push(
//                 xous::create_thread(move || {
//                     let msg = xous::receive_message(server).expect("couldn't receive message");
//                     assert_eq!(msg.message, xous::Message::Scalar(msg_contents));
//                 })
//                 .expect("couldn't spawn client thread"),
//             );
//         }

//         for _ in &server_threads {
//             xous::send_message(connection, xous::Message::Scalar(msg_contents))
//                 .expect("couldn't send message");
//         }
//         for server_thread in server_threads.into_iter() {
//             xous::wait_thread(server_thread).expect("couldn't wait for thread");
//         }
//     });

//     internal_server
//         .join()
//         .expect("couldn't join internal_server process");

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }

// #[test]
// fn multiple_multiple_contexts() {
//     for _ in 0..5 {
//         multiple_contexts();
//     }
// }

// /// Test that a server can be restarted and the kernel doesn't crash
// #[test]
// fn process_restart_server() {
//     let test_str = "Hello, world!";
//     let test_bytes = test_str.as_bytes();

//     let main_thread = start_kernel(SERVER_SPEC);

//     fn create_destroy_server(test_bytes: &'static [u8]) {
//         let (server_addr_send, server_addr_recv) = channel();

//         let xous_server = as_process(move || {
//             let sid =
//                 xous::create_server(b"test_recreate_se").expect("couldn't create test server");
//             server_addr_send.send(sid).unwrap();
//             let thr = xous::create_thread(move || {
//                 let envelope = xous::receive_message(sid).expect("couldn't receive messages");
//                 // println!("Received message from {}", envelope.sender);
//                 let message = envelope.message;
//                 if let xous::Message::Move(m) = message {
//                     let buf = m.buf;
//                     let bt = unsafe {
//                         Box::from_raw(core::slice::from_raw_parts_mut(buf.as_mut_ptr(), buf.len()))
//                     };
//                     assert_eq!(*test_bytes, *bt);
//                 // let s = String::from_utf8_lossy(&bt);
//                 // println!("Got message: {:?} -> \"{}\"", bt, s);
//                 } else {
//                     panic!("unexpected message type");
//                 }
//             })
//             .unwrap();
//             xous::wait_thread(thr).unwrap();
//         });

//         // Wait for the server to start up
//         let sid = server_addr_recv.recv().unwrap();

//         let conn = xous::connect(sid).expect("couldn't connect to server");
//         let msg = xous::carton::Carton::from_bytes(test_bytes);
//         xous::send_message(conn, xous::Message::Move(msg.into_message(0)))
//             .expect("couldn't send a message");
//         xous_server.join().expect("couldn't join server process");
//     }

//     // create_destroy_server(test_bytes);
//     create_destroy_server(test_bytes);

//     // Any process ought to be able to shut down the system currently.
//     rsyscall(SysCall::Shutdown).expect("unable to shutdown server");

//     main_thread.join().expect("couldn't join kernel process");
// }
