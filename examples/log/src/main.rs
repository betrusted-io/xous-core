#![cfg_attr(baremetal, no_std)]
#![cfg_attr(baremetal, no_main)]

#[cfg(baremetal)]
#[macro_use]
mod debug;

mod log_string;
mod start;

use log_string::LogString;
use core::fmt::Write;

#[cfg(not(baremetal))]
mod native_nostd {
    pub fn init() {}
}

#[cfg(baremetal)]
mod native_nostd {
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

        while let Some(c) = crate::debug::DEFAULT.getc() {
            print!("0x{:02x}", c);
        }
        println!();
    }

    pub fn init() {
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(0xf000_4000),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map serial port");
        unsafe { crate::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() };
        println!("Mapped UART @ {:08x}", uart.addr.get());

        println!("Process: map success!");
        crate::debug::DEFAULT.enable_rx();

        println!("Allocating IRQ...");
        xous::syscall::claim_interrupt(4, handle_irq, core::ptr::null_mut::<usize>())
            .expect("couldn't claim interrupt");
    }
}

#[cfg_attr(baremetal, no_mangle)]
fn main() {
    if cfg!(baremetal) {
        native_nostd::init();
    }

    println!("Starting server...");
    let server_addr =
        xous::syscall::create_server(xous::make_name!("📜")).expect("couldn't create server");
    println!("Server listening on address {:?}", server_addr);

    let mut counter: usize = 0;
    loop {
        if counter.trailing_zeros() >= 12 {
            println!("Counter tick: {}", counter);
        }
        counter += 1;
        println!("Waiting for an event...");
        let mut envelope =
            xous::syscall::receive_message(server_addr).expect("couldn't get address");
        println!("Got message envelope: {:?}", envelope);
        match &mut envelope.message {
            xous::Message::Scalar(msg) => {
                println!("Scalar message from {}: {:?}", envelope.sender, msg)
            }
            xous::Message::Move(msg) => {
                let log_entry = LogString::from_message(msg);
                println!("Moved log  message from {}: {}", envelope.sender, log_entry);
            }
            xous::Message::ImmutableBorrow(msg) => {
                let log_entry = LogString::from_message(msg);
                println!("Immutably borrowed log message from {}: {}", envelope.sender, log_entry);
            }
            xous::Message::MutableBorrow(msg) => {
                let mut log_entry = LogString::from_message(msg);
                println!("Immutably borrowed log message from {}: {}", envelope.sender, log_entry);
                writeln!(log_entry, " << HELLO FROM THE SERVER").unwrap();
            }
        }
    }
}
