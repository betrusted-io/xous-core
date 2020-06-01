#![no_std]
#![no_main]

#[macro_use]
mod debug;
mod log_string;
mod start;

use core::panic::PanicInfo;
use core::fmt::Write;
use log_string::LogString;

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
    println!();
}

#[no_mangle]
fn main() {
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf000_4000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    unsafe { debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() };
    println!("Mapped UART @ {:08x}", uart.addr.get());

    println!("Process: map success!");
    debug::DEFAULT.enable_rx();

    println!("Allocating IRQ...");
    xous::syscall::claim_interrupt(4, handle_irq, 0 as *mut usize)
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
    //         range.base as usize,
    //         range.base as usize + range.size
    //     );
    //     use core::slice;
    //     let mem_range = unsafe { slice::from_raw_parts_mut(range.base, range.size) };
    //     println!("Filling with bytes...");
    //     for word in mem_range.iter_mut() {
    //         *word = 42;
    //     }
    //     println!("Done!");
    // } else {
    //     println!("Unexpected return value: {:?}", heap);
    // }

    println!("Starting server...");
    let server_addr =
        xous::syscall::create_server(xous::make_name!("📜")).expect("couldn't create server");
    println!("Server listening on address {:?}", server_addr);

    let mut counter = 0;
    loop {
        if counter & 0xfff == 0 {
            println!("Counter tick: {}", counter);
        }
        counter += 1;
        // // println!("Waiting for an event...");
        // let mut envelope = xous::syscall::receive_message(server_addr).expect("couldn't get address");
        // // println!("Got message envelope: {:?}", envelope);
        // match &mut envelope.message {
        //     xous::Message::Scalar(msg) => {
        //         println!("Scalar message from {}: {:?}", envelope.sender, msg)
        //     }
        //     xous::Message::Move(msg) => {
        //         let log_entry = LogString::from_message(msg);
        //         println!("Moved log  message from {}: {}", envelope.sender, log_entry);
        //     }
        //     xous::Message::ImmutableBorrow(msg) => {
        //         let log_entry = LogString::from_message(msg);
        //         println!("Immutably borrowed log message from {}: {}", envelope.sender, log_entry);
        //     }
        //     xous::Message::MutableBorrow(msg) => {
        //         let mut log_entry = LogString::from_message(msg);
        //         println!("Immutably borrowed log message from {}: {}", envelope.sender, log_entry);
        //         writeln!(log_entry, " << HELLO FROM THE SERVER").unwrap();
        //     }
        // }

    }
}
