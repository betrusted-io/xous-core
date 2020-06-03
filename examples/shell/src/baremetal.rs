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

/// Rust entry point (_start_rust)
///
/// Zeros bss section, initializes data section and calls main. This function
/// never returns.
#[link_section = ".init.rust"]
#[export_name = "_start"]
pub unsafe extern "C" fn start_rust() -> ! {
    extern "Rust" {
        // This symbol will be provided by the kernel
        fn main();
    }

    let uart = xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf000_1000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .expect("couldn't map uart");
    unsafe { crate::debug::DEFAULT_UART_ADDR = uart.as_mut_ptr() };
    println!("Mapped UART @ {:08x}", uart.addr.get());

    xous::syscall::map_memory(
        xous::MemoryAddress::new(0xf000_2000),
        None,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    )
    .map(|_| println!("!!!WARNING: managed to steal kernel's memory"))
    .ok();
    println!("Process: map success!");

    crate::debug::DEFAULT.enable_rx();
    println!("Allocating IRQ...");
    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        2,
        handle_irq as *mut usize,
        core::ptr::null_mut::<usize>(),
    ))
    .expect("couldn't claim interrupt");

    main();
    panic!("exited main");
}

#[export_name = "abort"]
pub extern "C" fn abort() -> ! {
    panic!("aborted");
}
