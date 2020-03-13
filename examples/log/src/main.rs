#![no_std]
#![no_main]

#[macro_use]
mod debug;
mod start;

use core::panic::PanicInfo;

#[panic_handler]
fn handle_panic(arg: &PanicInfo) -> ! {
    println!("PANIC!");
    println!("Details: {:?}", arg);
    loop {}
}

fn handle_irq(irq_no: usize, arg: *mut usize) {
    print!("Handling IRQ {} (arg: {:08x}): ", irq_no, arg as usize);

    while let Some(c) = debug::DEFAULT.getc() {
        print!("0x{:02x}", c);
    }
    println!("");
}

#[no_mangle]
fn main() {
    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf0004000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map serial port");
    unsafe { debug::DEFAULT_UART_ADDR = uart.base as *mut usize };
    println!("Mapped UART @ {:08x}", uart.base as usize);

    println!("Process: map success!");
    debug::DEFAULT.enable_rx();
    println!("Allocating IRQ...");
    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        4,
        handle_irq as *mut usize,
        0 as *mut usize,
    ))
    .expect("couldn't claim interrupt");

    println!("Allocating a ton of space on the stack...");
    {
        let _big_array = [42u8; 131072];
    }

    println!("Increasing heap to 131072...");
    let heap = xous::rsyscall(xous::SysCall::IncreaseHeap(
        131072,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ))
    .expect("couldn't increase heap");
    if let xous::Result::MemoryRange(range) = heap {
        println!(
            "Heap goes from {:08x} - {:08x}",
            range.base as usize,
            range.base as usize + range.size
        );
        use core::slice;
        let mem_range = unsafe { slice::from_raw_parts_mut(range.base, range.size) };
        println!("Filling with bytes...");
        for word in mem_range.iter_mut() {
            *word = 42;
        }
        println!("Done!");
    } else {
        println!("Unexpected return value: {:?}", heap);
    }

    loop {
        println!("Waiting for an event...");
        xous::rsyscall(xous::SysCall::WaitEvent).expect("Couldn't call waitevent");
    }
}
