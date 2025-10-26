fn main() -> ! {
    // This boilerplate code sets up the logging infrastructure.
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("Hello world PID is {}", xous::process::id());

    // This delay of 5 seconds allows users watching over USB serial
    // console a few seconds to connect the console and see the output.
    // Otherwise, it will "print" to a console that's not listening.
    std::thread::sleep(std::time::Duration::from_secs(5));

    // This is the hello world!
    println!("Hello world!");

    // This ensures a graceful exit from the process.
    xous::terminate_process(0)
}
