#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use xous_ipc::String;

static XOUS_LOGGER: XousLogger = XousLogger {
    locked: AtomicBool::new(false),
};

struct XousLogger {
    locked: AtomicBool,
}

static mut XOUS_LOGGER_BACKING: XousLoggerBacking = XousLoggerBacking {
    conn: 0,
    initialized: false,
    buffer: None,
};

struct XousLoggerBacking {
    conn: xous::CID,
    buffer: Option<String<4000>>, // why 4000? tests non-power of 2 sizes in rkyv APIs. Could make it 4096 as well...
    initialized: bool,
}

impl XousLoggerBacking {
    fn init(&mut self) -> Result<(), xous::Error> {
        if self.initialized {
            return Ok(());
        }
        self.conn = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap())?;
        self.buffer = Some(String::<4000>::new());
        self.initialized = true;
        Ok(())
    }

    fn log_impl(&mut self, record: &log::Record) {
        if !self.initialized && self.init().is_err() {
            return;
        }
        if let Some(ref mut buf) = self.buffer {
            buf.clear();
            write!(buf, "{} - {}", record.level(), record.args()).unwrap();
            buf.lend(self.conn).unwrap();
        }
    }
}

impl log::Log for XousLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::Acquire)
            .is_err()
        {
            xous::yield_slice();
        }

        unsafe { XOUS_LOGGER_BACKING.log_impl(record) };
        self.locked
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::Acquire)
            .expect("LOG: logger became unlocked somehow");
    }
    fn flush(&self) {}
}

pub fn init() -> Result<(), log::SetLoggerError> {
    log::set_logger(&XOUS_LOGGER)?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

pub fn init_wait() -> Result<(), log::SetLoggerError> {
    log::set_logger(&XOUS_LOGGER)?;
    log::set_max_level(log::LevelFilter::Info);
    unsafe {
        while XOUS_LOGGER_BACKING.init().is_err() {
            xous::yield_slice();
        }
    }
    Ok(())
}
