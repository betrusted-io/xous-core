#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

extern crate ffi_sys;

mod api;
use api::*;
pub mod bindings;
pub use bindings::*;

use num_traits::FromPrimitive;

#[xous::xous_main]
fn xmain() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("my PID is {}", xous::process::id());

    log::info!("creating xns");
    let xns = xous_names::XousNames::new().unwrap();
    let mut a = 0;
    for _ in 0..5 {
        a = unsafe{add_one(a)};
        log::info!("ffi test: {}", a);
    }
    log::info!("malloc test result: {}", unsafe{malloc_test()});

    log::info!("registering with xns");
    let ffitest_sid = xns.register_name(api::SERVER_NAME_FFITEST, None).expect("can't register server");
    log::trace!("registered with NS -- {:?}", ffitest_sid);

    // spawn a small thread that keeps the watchdog timer from firing and lets us know other things didn't crash
    std::thread::spawn({
        move || {
            let tt = ticktimer_server::Ticktimer::new().unwrap();
            let mut state = 0;
            loop {
                tt.sleep_ms(2500).unwrap();
                log::info!("keepalive {}", state);
                state += 1;
            }
        }
    });

    loop {
        let msg = xous::receive_message(ffitest_sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(Opcode::Quit) => {
                break
            }
            _ => {
                log::info!("couldn't convert opcode {:?}", msg);
            }
        }
    }
    // clean up our program
    log::trace!("main loop exit, destroying servers");
    xns.unregister_server(ffitest_sid).unwrap();
    xous::destroy_server(ffitest_sid).unwrap();
    log::trace!("quitting");
    xous::terminate_process(0)
}
