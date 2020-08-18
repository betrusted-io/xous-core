#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
mod debug;

mod timer;
// mod logstr;

// fn print_and_yield(index: *mut usize) -> ! {
//     let num = index as usize;
//     loop {
//         println!("THREAD {}", num);
//         xous::syscall::yield_slice();
//     }
// }

#[xous::xous_main]
fn shell_main() -> ! {
    timer::init();

    let mut connection = None;

    let sid = xous::SID::from_bytes(b"xous-logs-output").unwrap();

    println!("Attempting to connect to server...");
    while connection.is_none() {
        if let Ok(cid) = xous::syscall::connect(sid) {
            connection = Some(cid);
        } else {
            xous::syscall::yield_slice();
        }
    }
    let connection = connection.unwrap();
    println!("Connected: {:?}", connection);

    let mut counter: usize = 0;
    // let ls = logstr::LogStr::new();
    loop {
        println!("Sending a scalar message with id {}...", counter + 4096);
        match xous::syscall::send_message(
            connection,
            xous::Message::Scalar(xous::ScalarMessage {
                id: counter + 4096,
                arg1: counter,
                arg2: counter * 2,
                arg3: !counter,
                arg4: counter + 1,
            }),
        ) {
            Err(xous::Error::ServerQueueFull) => {
                println!("Server queue is full... retrying");
                continue;
            },
            Ok(_) => {
                println!("Loop {}", counter);
                counter += 1;
            },
            Err(e) => panic!("Unable to send message: {:?}", e),
        }

        // #[cfg(not(target_os = "none"))]
        // std::thread::sleep(std::time::Duration::from_millis(500));
        // if counter & 2 == 0 {
        //     xous::syscall::yield_slice();
        // }

        // ls.clear();
        // write!(ls, "Hello, Server!  This memory is borrowed from another process.  Loop number: {}", counter).expect("couldn't send hello message");

        // println!("Sending a mutable borrow message");
        // let response = xous::syscall::send_message(
        //     connection,
        //     xous::Message::MutableBorrow(
        //         ls.as_memory_message(0)
        //             .expect("couldn't form memory message"),
        //     ),
        // )
        // .expect("couldn't send memory message");
        // unsafe { ls.set_len(response.0)};
        // println!("Message came back with args ({}, {}) as: {}", response.0, response.1, ls);
    }
}
