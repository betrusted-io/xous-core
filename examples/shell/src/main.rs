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

fn move_lfsr(mut lfsr: u32) -> u32 {
    lfsr ^= lfsr >> 7;
    lfsr ^= lfsr << 9;
    lfsr ^= lfsr >> 13;
    lfsr
}

fn ensure_connection(server: xous::SID) -> xous::CID {
    loop {
        if let Ok(cid) = xous::syscall::try_connect(server) {
            return cid;
        }
        xous::syscall::yield_slice();
    }
}

#[xous::xous_main]
fn shell_main() -> ! {
    timer::init();

    // let log_server_id = xous::SID::from_bytes(b"xous-logs-output").unwrap();
    let graphics_server_id = xous::SID::from_bytes(b"graphics-server ").unwrap();

    println!("SHELL: Attempting to connect to servers...");
    let log_conn = 100;//ensure_connection(log_server_id);
    let graphics_conn = ensure_connection(graphics_server_id);

    println!(
        "SHELL: Connected to Log server: {}  Graphics server: {}",
        log_conn, graphics_conn
    );

    assert_ne!(
        log_conn, graphics_conn,
        "SHELL: graphics and log connections are the same!"
    );

    let mut counter: usize = 0;
    // let ls = logstr::LogStr::new();
    let mut lfsr = 0xace1u32;
    let dark = graphics_server::Color::from(0);
    let light = graphics_server::Color::from(!0);
    loop {
        // println!("Sending a scalar message with id {}...", counter + 4096);
        // match xous::syscall::send_message(
        //     log_conn,
        //     xous::Message::Scalar(xous::ScalarMessage {
        //         id: counter + 4096,
        //         arg1: counter,
        //         arg2: counter * 2,
        //         arg3: !counter,
        //         arg4: counter + 1,
        //     }),
        // ) {
        //     Err(xous::Error::ServerQueueFull) => {
        //         // println!("Server queue is full... retrying");
        //         continue;
        //     }
        //     Ok(_) => {
        //         println!("Loop {}", counter);
        //         counter += 1;
        //     }
        //     Err(e) => panic!("Unable to send message: {:?}", e),
        // }

        // lfsr = move_lfsr(lfsr);

        loop {
            match graphics_server::set_style(
                graphics_conn,
                5,
                if lfsr & 1 == 0 { dark } else { light },
                if lfsr & 1 == 0 { dark } else { light },
            ) {
                Err(xous::Error::ServerQueueFull) => continue,
                Ok(_) => break,
                Err(e) => panic!("unable to draw to screen: {:?}", e),
            }
        }
        let x1 = move_lfsr(lfsr);
        let y1 = move_lfsr(x1);
        let x2 = move_lfsr(y1);
        let y2 = move_lfsr(x2);
        lfsr = y2;

        loop {
            match graphics_server::draw_line(
                graphics_conn,
                graphics_server::Point::new((x1 % 336) as _, (y1 % 536) as _),
                graphics_server::Point::new((x2 % 336) as _, (y2 % 536) as _),
            ) {
                Err(xous::Error::ServerQueueFull) => continue,
                Ok(_) => break,
                Err(e) => panic!("unable to draw to screen: {:?}", e),
            }
        }

        loop {
            match graphics_server::flush(graphics_conn) {
                Err(xous::Error::ServerQueueFull) => continue,
                Ok(_) => break,
                Err(e) => panic!("unable to draw to screen: {:?}", e),
            }
        }

        // let lfsr = move_lfsr(lfsr);
        // if lfsr.trailing_zeros() >= 3 {
        //     loop {
        //         match xous::syscall::try_send_message(
        //             log_conn,
        //             xous::Message::Scalar(xous::ScalarMessage {
        //                 id: counter + 4096,
        //                 arg1: counter,
        //                 arg2: counter * 2,
        //                 arg3: !counter,
        //                 arg4: lfsr as _,
        //             }),
        //         ) {
        //             Err(xous::Error::ServerQueueFull) => {
        //                 println!("SHELL: Log Server queue is full... retrying");
        //                 continue;
        //             }
        //             Ok(_) => {
        //                 println!("SHELL: Loop {}", counter);
        //                 counter += 1;
        //                 break;
        //             }
        //             Err(e) => panic!("Unable to send message: {:?}", e),
        //         }
        //     }
        // }
        // // #[cfg(not(target_os = "none"))]
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
