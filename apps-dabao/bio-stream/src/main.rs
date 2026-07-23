use core::sync::atomic::{AtomicU32, Ordering};

use usb_bao1x::{SERIAL_BINARY_BUFLEN, UsbDeviceState, UsbHid};

const START_DELAY_MS: usize = 3000;
const TX_BLOCK_SIZE: usize = SERIAL_BINARY_BUFLEN;
const REPORT_INTERVAL_MS: usize = 1000;

// Number of bytes successfully transmitted during the last one-second interval.
static BYTES_INTERVAL: AtomicU32 = AtomicU32::new(0);

// Return value statistics for the last one-second interval.
static OK_INTERVAL: AtomicU32 = AtomicU32::new(0);
static ZERO_INTERVAL: AtomicU32 = AtomicU32::new(0);
static ERROR_INTERVAL: AtomicU32 = AtomicU32::new(0);

fn make_test_pattern() -> [u8; TX_BLOCK_SIZE] {
    let mut data = [0u8; TX_BLOCK_SIZE];

    let mut index = 0usize;
    while index < TX_BLOCK_SIZE {
        data[index] = index as u8;
        index += 1;
    }

    data
}

fn wait_until_usb_configured(usb: &UsbHid, timer: &ticktimer::Ticktimer) {
    while usb.status() != UsbDeviceState::Configured {
        timer.sleep_ms(10).ok();
    }
}

/// Reports the application-side accepted throughput once per second.
///
/// This measures bytes accepted by `UsbHid::serial_send()`. It does not
/// directly measure bytes delivered to the host.
fn start_reporter_thread() {
    std::thread::spawn(|| {
        let timer = ticktimer::Ticktimer::new().expect("reporter could not connect to ticktimer");

        let mut report_no = 0u32;

        loop {
            timer.sleep_ms(REPORT_INTERVAL_MS).ok();
            report_no = report_no.wrapping_add(1);

            let bytes = BYTES_INTERVAL.swap(0, Ordering::Relaxed);
            let ok = OK_INTERVAL.swap(0, Ordering::Relaxed);
            let zero = ZERO_INTERVAL.swap(0, Ordering::Relaxed);
            let errors = ERROR_INTERVAL.swap(0, Ordering::Relaxed);

            log::info!(
                "USB TX r={} accepted_Bps={} writes={} zero={} errors={}",
                report_no,
                bytes,
                ok,
                zero,
                errors,
            );
        }
    });
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let timer = ticktimer::Ticktimer::new().expect("could not connect to ticktimer");

    let usb = UsbHid::new();

    log::info!("USB CDC transmit stress test started: PID={}", xous::process::id());

    log::info!("waiting for USB configuration");
    wait_until_usb_configured(&usb, &timer);

    log::info!("USB configured; transmission starts in {} ms", START_DELAY_MS);

    start_reporter_thread();

    timer.sleep_ms(START_DELAY_MS).ok();

    let tx_data = make_test_pattern();

    loop {
        let mut offset = 0usize;

        while offset < tx_data.len() {
            match usb.serial_send(&tx_data[offset..]) {
                Ok(0) => {
                    ZERO_INTERVAL.fetch_add(1, Ordering::Relaxed);

                    // Avoid starving the USB service when its transmit buffer
                    // cannot currently accept more data.
                    xous::yield_slice();
                }

                Ok(sent) => {
                    offset += sent;

                    BYTES_INTERVAL.fetch_add(sent as u32, Ordering::Relaxed);
                    OK_INTERVAL.fetch_add(1, Ordering::Relaxed);
                }

                Err(_) => {
                    ERROR_INTERVAL.fetch_add(1, Ordering::Relaxed);

                    // Avoid a tight retry loop if the service is temporarily
                    // unavailable.
                    xous::yield_slice();
                }
            }
        }
    }
}
