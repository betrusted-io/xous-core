use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::Duration;

use log::info;

// put here to force a .sbss/.bss section for loader testing
static mut LOOP_COUNT: AtomicU32 = AtomicU32::new(0);

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("my PID is {}", xous::process::id());

    const DELAY_MS: u64 = 1000;

    for i in 0.. {
        unsafe { LOOP_COUNT.store(i, Ordering::SeqCst) };
        info!("Loop #{}, waiting {} ms", unsafe { LOOP_COUNT.load(Ordering::SeqCst) }, DELAY_MS);
        sleep(Duration::from_millis(DELAY_MS));
    }

    panic!("Finished endless loop");
}
