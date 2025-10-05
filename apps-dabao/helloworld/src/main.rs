fn main() -> ! {
    // This boilerplate code sets up the logging infrastructure.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Hello world PID is {}", xous::process::id());

    println!("Hello world");

    // This ensures a graceful exit from the process.
    xous::terminate_process(0)
}
