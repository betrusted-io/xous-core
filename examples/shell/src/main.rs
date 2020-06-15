#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
mod debug;

#[cfg(baremetal)]
mod baremetal;

mod timer;
// mod logstr;

#[cfg(baremetal)]
use core::fmt::Write;

// fn print_and_yield(index: *mut usize) -> ! {
//     let num = index as usize;
//     loop {
//         println!("THREAD {}", num);
//         xous::syscall::yield_slice();
//     }
// }

#[cfg_attr(baremetal, no_mangle)]
fn main() {
    println!("Starting to initialize the timer");
    timer::init();

    let mut connection = None;
    println!("Attempting to connect to server...");
    while connection.is_none() {
        if let Ok(cid) = xous::syscall::connect((1, 2_626_920_432, 1, 2_626_920_432)) {
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
        xous::syscall::send_message(
            connection,
            xous::Message::Scalar(xous::ScalarMessage {
                id: counter + 4096,
                arg1: counter,
                arg2: counter * 2,
                arg3: !counter,
                arg4: counter + 1,
            }),
        )
        .expect("couldn't send scalar message");
        if counter.trailing_zeros() >= 12 {
            println!("Loop {}", counter);
        }
        counter += 1;
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
