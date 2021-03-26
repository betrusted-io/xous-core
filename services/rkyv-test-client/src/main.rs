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
    let double_src = xous::String::<256>::from_str("12345678");

    let mut message_string = xous::String::<64>::new();
    loop {
        log::info!("2 + {} = {}", idx, rkyv_test_server::add(2, idx).unwrap());
        ticktimer_server::sleep_ms(ticktimer_conn, 500).ok();

        message_string.clear();
        write!(message_string, "I'm at loop # {:^4}", idx);
        rkyv_test_server::log_message("prefix", message_string);
        log::info!("Doubling string {}", double_src);
        log::info!("Doubled string: {}", rkyv_test_server::double_string(&double_src));
        idx += 1;
    }
}
