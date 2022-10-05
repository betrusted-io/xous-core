# Xous API: Log

API calls to access the Xous logging service. Provides glue between the `log` crate, the Xous kernel and the hardware.

Every process that relies on the logging service should call `xous_api_log::init_wait()` before using any `log` calls.

Below is a minimal example of how to use the logging service.

```rust
fn main() -> ! {
    xous_api_log::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("my PID is {}", xous::process::id());

    let timeout = std::time::Duration::from_millis(1000);
    let mut count = 0;
    loop {
        log::info!("test loop {}", count);
        count += 1;
        std::thread::sleep(timeout);
    }
}
```
