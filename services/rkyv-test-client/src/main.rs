#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

use core::fmt::Write;

fn print_log_messages(log_message: &str, prefix: &str) {
    log::info!(
        "Got a message hook! Message: [{}], prefix: [{}]",
        log_message,
        prefix
    );
}

fn handle_battstats(bs: com::BattStats) {
    log::info!("battstats: {:?}", bs);
    // note: at this point to repatriate data into the core main loop, you have two options:
    // 1. send a new message to the server, using your own private API. This feels "wasteful"
    //    as you're bouncing a message twice but it keeps the API spaces strictly on crate boundaries
    // 2. use an Atomic type to transfer primitive data types from the handler thread to the main thread
}

fn handle_keyevents(keys: [char; 4]) {
    for &k in keys.iter() {
        if k != '\u{0000}' {
            log::info!("KEYEVENT: {}", k);
        }
    }
}

#[xous::xous_main]
fn rkyv_test_client() -> ! {
    log_server::init_wait().unwrap();
    log::info!(
        "Hello, world! This is the client, PID {}",
        xous::current_pid().unwrap().get()
    );
    let ticktimer = ticktimer_server::Ticktimer::new().expect("couldn't create ticktimer object");

    rkyv_test_server::hook_log_messages(print_log_messages);

    let mut idx = 0;

    // This is just some number. It should change width, ensuring our padding
    // is working and we're not overwriting the buffer weirdly.
    let mut some_number = 98653i32;
    let mut double_src = xous::String::<256>::new();

    let mut message_string = xous::String::<64>::new();

    let xns = xous_names::XousNames::new().unwrap();

    let mut susres = susres::Susres::new_without_hook(&xns).unwrap();
    let mut com = com::Com::new(&xns).unwrap();
    com.hook_batt_stats(handle_battstats).unwrap();
    /*
    let mut kbd = keyboard::Keyboard::new(&xns).unwrap();
    kbd.hook_keyboard_events(handle_keyevents).unwrap();*/
    loop {
        log::info!("2 + {} = {}", idx, rkyv_test_server::add(2, idx).unwrap());
        ticktimer.sleep_ms(3000).ok();

        message_string.clear();
        write!(message_string, "I'm at loop # {:^4} (some numer: {})", idx, some_number).unwrap();
        some_number = some_number.wrapping_mul(53);
        rkyv_test_server::log_message("prefix", message_string);

        double_src.clear();
        write!(double_src, "12345678 Loop {} 🎉 你好", idx).unwrap();
        log::info!("Doubling string {}", double_src);
        log::info!("Doubled string: {}", rkyv_test_server::double_string(&double_src));

        let sent_str = xous::String::<32>::from_str("This got moved");
        log::info!("Sending a string \"{}\"", sent_str);
        rkyv_test_server::log_message("prefix", sent_str);

        com.req_batt_stats().unwrap();

        // let the loop run a bit, then try a suspend
        if idx == 2 {
            // TODO: add a self-wakeup RTC alarm once we're beyond the touch-and-go phase
            susres.initiate_suspend().unwrap();
        }

        idx += 1;
    }
}

