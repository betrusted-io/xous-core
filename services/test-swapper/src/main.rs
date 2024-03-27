use std::thread::sleep;
use std::time::Duration;

use log::info;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("my PID is {}", xous::process::id());

    const DELAY_MS: u64 = 2000;

    for i in 0.. {
        info!("Loop #{}, waiting {} ms", i, DELAY_MS);
        sleep(Duration::from_millis(DELAY_MS));
    }

    panic!("Finished endless loop");
}
