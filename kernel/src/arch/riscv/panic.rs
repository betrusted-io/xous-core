use core::panic::PanicInfo;

use crate::arch;

#[cfg(baremetal)]
#[panic_handler]
fn handle_panic(_arg: &PanicInfo) -> ! {
    println!("PANIC in PID {}: {}", crate::arch::current_pid(), _arg);
    #[cfg(any(feature = "precursor", feature = "renode"))]
    {
        use core::fmt::Write;

        use crate::platform::precursor::lcdpanic::ErrorWriter;
        if let Ok(mut writer) = ErrorWriter::new() {
            writeln!(writer, "PANIC in PID {}: {}", crate::arch::current_pid(), _arg).ok();
            let process = crate::arch::process::Process::current();
            writeln!(writer, "Current thread: {}", process.current_tid()).ok();
            writeln!(writer, "{}", crate::arch::process::Process::current().current_thread()).ok();
        }
    }
    loop {
        arch::idle();
    }
}
