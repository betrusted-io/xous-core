use keystore_api::*;

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let xns = xous_names::XousNames::new().unwrap();
    // TODO: limit connections to this server?? once we know all that will connect
    let keys_sid = xns.register_name(SERVER_NAME_KEYS, None).expect("can't register server");

    loop {}
}
