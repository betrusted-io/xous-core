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

#[xous::xous_main]
fn rkyv_test_client() -> ! {
    log_server::init_wait().unwrap();
    log::info!(
        "Hello, world! This is the client, PID {}",
        xous::current_pid().unwrap().get()
    );
    let ticktimer_server_id = xous::SID::from_bytes(b"ticktimer-server").unwrap();
    let ticktimer_conn = xous::connect(ticktimer_server_id).unwrap();

    rkyv_test_server::hook_log_messages(print_log_messages);

    let mut idx = 0;

    // This is just some number. It should change width, ensuring our padding
    // is working and we're not overwriting the buffer weirdly.
    let mut some_number = 98653i32;
    let mut double_src = xous::String::<256>::new();

    let mut message_string = xous::String::<64>::new();
    loop {
        log::info!("2 + {} = {}", idx, rkyv_test_server::add(2, idx).unwrap());
        ticktimer_server::sleep_ms(ticktimer_conn, 500).ok();

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

        idx += 1;
    }
}

