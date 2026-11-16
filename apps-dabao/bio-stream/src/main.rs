//! USB CDC transmit stress test for Dabao.
//!
//! Sends a continuous stream of 512-byte records framed as:
//!   magic(2) + seq(2) + payload(504) + crc32(4)
//! so a host can detect whole-packet duplicates (xous-core#1017).
//!
//! Host: `python tools/usb_dup_detect.py --port COMx --framed-seconds 5`

use core::sync::atomic::{AtomicU32, Ordering};

use usb_bao1x::{UsbDeviceState, UsbHid};

const START_DELAY_MS: usize = 3000;
const REPORT_INTERVAL_MS: usize = 1000;
const REC: usize = 512;
const REC_PAYLOAD: usize = REC - 8;
const MAGIC: [u8; 2] = [0xb5, 0x5c];

static BYTES_INTERVAL: AtomicU32 = AtomicU32::new(0);
static OK_INTERVAL: AtomicU32 = AtomicU32::new(0);
static ZERO_INTERVAL: AtomicU32 = AtomicU32::new(0);
static ERROR_INTERVAL: AtomicU32 = AtomicU32::new(0);

fn crc32(data: &[u8]) -> u32 {
    // IEEE CRC-32 (same polynomial as zlib / baoscope framing)
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn fill_record(buf: &mut [u8; REC], seq: u16) {
    buf[0] = MAGIC[0];
    buf[1] = MAGIC[1];
    buf[2] = (seq & 0xff) as u8;
    buf[3] = (seq >> 8) as u8;
    for i in 0..REC_PAYLOAD {
        // Non-repeating payload so a duplicated 512B packet is visible even
        // without CRC (seq alone is enough; CRC catches corruption).
        buf[4 + i] = ((seq as usize).wrapping_mul(131).wrapping_add(i)) as u8;
    }
    let c = crc32(&buf[..REC - 4]);
    buf[REC - 4] = (c & 0xff) as u8;
    buf[REC - 3] = ((c >> 8) & 0xff) as u8;
    buf[REC - 2] = ((c >> 16) & 0xff) as u8;
    buf[REC - 1] = ((c >> 24) & 0xff) as u8;
}

fn wait_until_usb_configured(usb: &UsbHid, timer: &ticktimer::Ticktimer) {
    while usb.status() != UsbDeviceState::Configured {
        timer.sleep_ms(10).ok();
    }
}

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

fn send_all(usb: &UsbHid, data: &[u8]) -> Result<(), ()> {
    let mut offset = 0usize;
    while offset < data.len() {
        match usb.serial_send(&data[offset..]) {
            Ok(0) => {
                ZERO_INTERVAL.fetch_add(1, Ordering::Relaxed);
                xous::yield_slice();
            }
            Ok(sent) => {
                offset += sent;
                BYTES_INTERVAL.fetch_add(sent as u32, Ordering::Relaxed);
                OK_INTERVAL.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                ERROR_INTERVAL.fetch_add(1, Ordering::Relaxed);
                xous::yield_slice();
                return Err(());
            }
        }
    }
    Ok(())
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);

    let timer = ticktimer::Ticktimer::new().expect("could not connect to ticktimer");
    let usb = UsbHid::new();

    log::info!("USB CDC framed stress test (#1017): PID={}", xous::process::id());
    log::info!("waiting for USB configuration");
    wait_until_usb_configured(&usb, &timer);
    log::info!("USB configured; transmission starts in {} ms", START_DELAY_MS);

    start_reporter_thread();
    timer.sleep_ms(START_DELAY_MS).ok();

    let mut rec = [0u8; REC];
    let mut seq: u16 = 0;

    loop {
        fill_record(&mut rec, seq);
        if send_all(&usb, &rec).is_ok() {
            seq = seq.wrapping_add(1);
        }
    }
}
