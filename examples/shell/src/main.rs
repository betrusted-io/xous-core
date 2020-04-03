#![no_std]
#![no_main]

#[macro_use]
mod debug;
mod start;

mod timer;

mod logstr;

use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn handle_panic(arg: &PanicInfo) -> ! {
    println!("PANIC!");
    println!("Details: {:?}", arg);
    xous::syscall::wait_event();
    loop {}
}

fn handle_irq(irq_no: usize, arg: *mut usize) {
    print!("Handling IRQ {} (arg: {:08x}): ", irq_no, arg as usize);

    while let Some(c) = debug::DEFAULT.getc() {
        print!("0x{:02x}", c);
    }
    println!("");
}

// fn print_and_yield(index: *mut usize) -> ! {
//     let num = index as usize;
//     loop {
//         println!("THREAD {}", num);
//         xous::syscall::yield_slice();
//     }
// }

#[no_mangle]
fn main() {
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf0001000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map uart");
    unsafe { debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() };
    println!("Mapped UART @ {:08x}", uart.addr.get() );

    xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf0002000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .map(|_| println!("!!!WARNING: managed to steal kernel's memory"))
    .ok();
    println!("Process: map success!");

    debug::DEFAULT.enable_rx();
    println!("Allocating IRQ...");
    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        2,
        handle_irq as *mut usize,
        0 as *mut usize,
    ))
    .expect("couldn't claim interrupt");

    // println!("Allocating a ton of space on the stack...");
    // {
    //     let _big_array = [42u8; 131072];
    // }

    // println!("Increasing heap to 131072...");
    // let heap = xous::rsyscall(xous::SysCall::IncreaseHeap(
    //     131072,
    //     xous::MemoryFlags::R | xous::MemoryFlags::W,
    // ))
    // .expect("couldn't increase heap");
    // if let xous::Result::MemoryRange(range) = heap {
    //     println!(
    //         "Heap goes from {:08x} - {:08x}",
    //         range.addr.get(),
    //         range.addr.get() + range.size.get()
    //     );
    //     use core::slice;
    //     let mem_range = unsafe { slice::from_raw_parts_mut(range.as_mut_ptr() as *mut u8, range.len()) };
    //     println!("Filling with bytes...");
    //     for word in mem_range.iter_mut() {
    //         *word = 42;
    //     }
    //     println!("Done!");
    // } else {
    //     println!("Unexpected return value: {:?}", heap);
    // }

    timer::init();

    // for i in 1usize..5 {
    //     xous::rsyscall(xous::SysCall::SpawnThread(
    //         print_and_yield as *mut usize,
    //         (0x8000_0000 - 32768 - i * 4096) as *mut usize,
    //         i as *mut usize,
    //     )).expect("couldn't spawn thread");
    // }
    // loop {
    //     println!("main thread waiting...");
    //     xous::syscall::wait_event();
    //     println!("MAIN THREAD WOKE UP");
    // }

    let mut connection = None;
    println!("Attempting to connect to server...");
    while connection.is_none() {
        if let Ok(cid) = xous::syscall::connect((3, 2626920432, 3, 2626920432)) {
            connection = Some(cid);
        } else {
            xous::syscall::yield_slice();
        }
    }
    let connection = connection.unwrap();
    println!("Connected: {:?}", connection);

    let mut counter = 0;
    let mut ls = logstr::LogStr::new();
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
        counter += 1;
        if counter & 2 == 0 {
            xous::syscall::yield_slice();
        }

        ls.clear();
        write!(ls, "Hello, Server!  This memory is borrowed from another process.  Loop number: {}", counter).expect("couldn't send hello message");

        println!("Sending a mutable borrow message");
        let response = xous::syscall::send_message(
            connection,
            xous::Message::MutableBorrow(
                ls.as_memory_message(0)
                    .expect("couldn't form memory message"),
            ),
        )
        .expect("couldn't send memory message");
        unsafe { ls.set_len(response.0)};
        println!("Message came back with args ({}, {}) as: {}", response.0, response.1, ls);
    }
}
