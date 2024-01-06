#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use log::info;
use std::thread::sleep;
use std::time::Duration;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("my PID is {}", xous::process::id());

    #[cfg(feature="susres-testing")]
    const DELAY_MS: u64 = 2000;
    #[cfg(not(feature="susres-testing"))]
    const DELAY_MS: u64 = 5000;

    #[cfg(feature="susres-testing")]
    let xns = xous_names::XousNames::new().unwrap();
    #[cfg(feature="susres-testing")]
    let susres = susres::Susres::new_without_hook(&xns).unwrap();

    for i in 0.. {
        info!("Loop #{}, waiting {} ms", i, DELAY_MS);
        sleep(Duration::from_millis(DELAY_MS));
        #[cfg(feature="susres-testing")]
        {
            info!("initiate suspend");
            susres.initiate_suspend().unwrap();
            info!("after suspend, sleeping");
            sleep(Duration::from_millis(DELAY_MS));
        }
    }

    panic!("Finished endless loop");
}
