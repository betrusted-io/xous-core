use core::panic::PanicInfo;
use armv7;

#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC (PID {}): {}", crate::arch::current_pid(), _arg);

    armv7::asm::bkpt(); // Invoke a debugger breakpoint

    loop {}
}
