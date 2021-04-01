#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use xous_ipc::{String, Buffer};

pub mod api;

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

struct XousLoggerBacking<'a> {
    conn: xous::CID,
    buffer: Option<Buffer<'a>>,
    initialized: bool,
}

impl XousLoggerBacking<'_> {
    fn init(&mut self) -> Result<(), xous::Error> {
        if self.initialized {
            return Ok(());
        }
        self.conn = xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap())?;
        self.buffer = Some(Buffer::new(4000)); // why 4000? tests non-power of 2 sizes in rkyv APIs. Could make it 4096 as well...
        self.initialized = true;
        Ok(())
    }

    fn log_impl(&mut self, record: &log::Record) {
        if !self.initialized && self.init().is_err() {
            return;
        }
        let mut args = String::<2800>::new();
        write!(args, "{}", record.args()).unwrap();
        let lr = api::LogRecord {
            file: String::from_str(record.file().unwrap_or("")),
            line: record.line(),
            module: String::from_str(record.module_path().unwrap_or("")),
            level: record.level() as u32,
            args,
        };

        if let Some(buf) = self.buffer.as_mut() {
            buf.rewrite(lr).unwrap();
            buf.lend(self.conn, 0).unwrap(); // there is only one type of buffer we should be sending!
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
