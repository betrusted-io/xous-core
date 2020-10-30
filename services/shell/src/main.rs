#![cfg_attr(baremetal, no_main)]
#![cfg_attr(baremetal, no_std)]

#[cfg(baremetal)]
#[macro_use]
mod debug;

mod logstr;
mod timer;
use core::fmt::Write;

// fn print_and_yield(index: *mut usize) -> ! {
//     let num = index as usize;
//     loop {
//         println!("THREAD {}", num);
//         xous::syscall::yield_slice();
//     }
// }

#[cfg(baremetal)]
use utralib::generated::*;

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

struct StringBuffer<'a> {
    raw_slice: &'a mut [u8],
    pub s: &'a str,
    pub len: usize,
}

impl<'a> StringBuffer<'a> {
    pub fn new(max: usize) -> StringBuffer<'a> {
        let mem = xous::map_memory(None, None, max, xous::MemoryFlags::R | xous::MemoryFlags::W).unwrap();
        let p = mem.as_mut_ptr();
        for i in 0..max {
            unsafe { p.add(i).write_volatile(0) };
        }
        StringBuffer {
            raw_slice: unsafe { core::slice::from_raw_parts_mut(mem.as_mut_ptr(), max)},
            s: unsafe {
                core::str::from_utf8_unchecked(core::slice::from_raw_parts(mem.as_ptr(), 0))
            },
            len: 0,
        }
    }

    /// Perform an immutable lend of this Carton to the specified server.
    /// This function will block until the server returns.
    pub fn lend(&self, connection: xous::CID, id: usize) -> Result<xous::Result, xous::Error> {
        let memory_range = xous::MemoryRange::new(self.raw_slice.as_ptr() as _, self.raw_slice.len()).unwrap();
        let msg = xous::MemoryMessage {
            id,
            buf: memory_range,
            offset: None,
            valid: xous::MemorySize::new(self.len).map(|x| Some(x)).unwrap_or(None),
        };
        xous::send_message(connection, xous::Message::Borrow(msg))
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.s = unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(self.raw_slice.as_ptr(), self.len))
        };
    }
}

impl<'a> core::fmt::Display for StringBuffer<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.s)
    }
}

impl<'a> core::fmt::Write for StringBuffer<'a> {
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        for c in s.bytes() {
            if self.len < self.raw_slice.len() {
                self.raw_slice[self.len] = c;
                self.len += 1;
            }
        }
        self.s = unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(self.raw_slice.as_ptr(), self.len))
        };
        Ok(())
    }
}


#[xous::xous_main]
fn shell_main() -> ! {
    timer::init();

    // let log_server_id = xous::SID::from_bytes(b"xous-logs-output").unwrap();
    let graphics_server_id = xous::SID::from_bytes(b"graphics-server ").unwrap();
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let log_server_id = xous::SID::from_bytes(b"xous-log-server ").unwrap();

    println!("SHELL: Attempting to connect to servers...");
    let log_conn = ensure_connection(log_server_id);
    let graphics_conn = ensure_connection(graphics_server_id);
    let ticktimer_conn = ensure_connection(ticktimer_server_id);

    println!(
        "SHELL: Connected to Log server: {}  Graphics server: {}  Ticktimer server: {}",
        log_conn, graphics_conn, ticktimer_conn,
    );

    assert_ne!(
        log_conn, graphics_conn,
        "SHELL: graphics and log connections are the same!"
    );

    assert_ne!(
        ticktimer_conn, graphics_conn,
        "SHELL: graphics and ticktimer connections are the same!"
    );

    // let mut counter: usize = 0;
    let mut ls = logstr::LogStr::new();
    let mut lfsr = 0xace1u32;
    let dark = graphics_server::Color::from(0);
    let light = graphics_server::Color::from(!0);

    #[cfg(baremetal)]
    {
        let gpio_base = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::gpio::HW_GPIO_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map GPIO CSR range");
        let mut gpio = CSR::new(gpio_base.as_mut_ptr() as *mut u32);
        gpio.wfo(utra::gpio::UARTSEL_UARTSEL, 2);
    }

    let mut last_time: u64 = 0;
    let mut string_buffer = StringBuffer::new(4096);
    loop {
        // a message passing demo -- checking time
        if let Ok(elapsed_time) = ticktimer_server::elapsed_ms(ticktimer_conn) {
            println!("SHELL: {}ms", elapsed_time);
            if elapsed_time - last_time > 40 {
                last_time = elapsed_time;
                /*
                xous::try_send_message(log_conn,
                    xous::Message::Scalar(xous::ScalarMessage{id:256, arg1: elapsed_time as usize, arg2: 257, arg3: 258, arg4: 259}));
                */
                println!("Preparing a mutable borrow message");

                ls.clear();
                write!(
                    ls,
                    "Hello, Server!  This memory is borrowed from another process.  Elapsed: {}",
                    elapsed_time as usize
                )
                .expect("couldn't send hello message");

                let mm = ls
                    .as_memory_message(0)
                    .expect("couldn't form memory message");

                println!("Sending a mutable borrow message");

                let response =
                    xous::syscall::try_send_message(log_conn, xous::Message::MutableBorrow(mm))
                        .expect("couldn't send memory message");
                //unsafe { ls.set_len(response.0)};
                //println!("Message came back with args ({}, {}) as: {}", response.0, response.1, ls);
            }
        } else {
            println!("error requesting ticktimer!")
        }

        graphics_server::set_style(
            graphics_conn,
            5,
            if lfsr & 1 == 0 { dark } else { light },
            if lfsr & 1 == 0 { dark } else { light },
        )
        .expect("unable to draw to screen: {:?}");

        let x1 = move_lfsr(lfsr);
        let y1 = move_lfsr(x1);
        let x2 = move_lfsr(y1);
        let y2 = move_lfsr(x2);
        lfsr = y2;

        graphics_server::draw_line(
            graphics_conn,
            graphics_server::Point::new((x1 % 336) as _, (y1 % 536) as _),
            graphics_server::Point::new((x2 % 336) as _, (y2 % 536) as _),
        )
        .expect("unable to draw to screen");

        string_buffer.clear();
        write!(&mut string_buffer, "Elapsed time: {}ms", last_time).expect("Can't write");
        graphics_server::clear_region(graphics_conn, 0, 0, 300, 40).expect("unable to clear region");
        string_buffer.lend(graphics_conn, 1).expect("unable to draw string");

        graphics_server::flush(graphics_conn).expect("unable to draw to screen");

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
    }
}
