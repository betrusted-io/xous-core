#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use num_traits::ToPrimitive;
use xous_ipc::{Buffer, String};

pub mod api;

#[derive(Debug)]
pub enum LogError {
    LoggerExists,
    NoConnection,
}

static XOUS_LOGGER: XousLogger = XousLogger {
    locked: AtomicBool::new(false),
};

struct XousLogger {
    locked: AtomicBool,
}

// static mut XOUS_LOGGER_BACKING: XousLoggerBacking = XousLoggerBacking {
//     conn: 0,
//     initialized: false,
//     buffer: None,
// };

static mut XOUS_LOGGER_BACKING: Option<XousLoggerBacking> = None;

struct XousLoggerBacking<'a> {
    conn: xous::CID,
    buffer: Buffer<'a>,
}

impl<'a> XousLoggerBacking<'a> {
    pub fn new() -> Result<Self, xous::Error> {
        Ok(XousLoggerBacking {
            conn: xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap())?,
            // why 4000? tests non-power of 2 sizes in rkyv APIs. Could make it 4096 as well...
            buffer: Buffer::new(4000),
        })
    }
}

impl Default for XousLoggerBacking<'_> {
    fn default() -> Self {
        XousLoggerBacking {
            conn: xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap(),
            // why 4000? tests non-power of 2 sizes in rkyv APIs. Could make it 4096 as well...
            buffer: Buffer::new(4000),
        }
    }
}

impl XousLoggerBacking<'_> {
    fn log_impl(&mut self, record: &log::Record) {
        let mut args = String::<2800>::new();
        write!(args, "{}", record.args()).unwrap();
        let lr = api::LogRecord {
            file: String::from_str(record.file().unwrap_or("")),
            line: record.line(),
            module: String::from_str(record.module_path().unwrap_or("")),
            level: record.level() as u32,
            args,
        };

        self.buffer.rewrite(lr).unwrap();
        self.buffer
            .lend(self.conn, crate::api::Opcode::LogRecord.to_u32().unwrap())
            .unwrap();
    }
    fn resume(&self) {
        xous::send_message(
            self.conn,
            xous::Message::new_scalar(2000, 0, 0, 0, 0), // logger is one of the few servers that uses special, non-encoded message IDs.
        )
        .expect("couldn't send resume message to the logger implementation");
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

        if unsafe { XOUS_LOGGER_BACKING.is_none() } {
            unsafe { XOUS_LOGGER_BACKING = Some(XousLoggerBacking::default()) };
        }
        unsafe { XOUS_LOGGER_BACKING.as_mut().unwrap().log_impl(record) };
        self.locked
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::Acquire)
            .expect("LOG: logger became unlocked somehow");
    }
    fn flush(&self) {}
}

pub fn init() -> Result<(), LogError> {
    if let Ok(backing) = XousLoggerBacking::new() {
        unsafe {
            XOUS_LOGGER_BACKING = Some(backing);
        }
        log::set_logger(&XOUS_LOGGER).map_err(|_| LogError::LoggerExists)?;
        log::set_max_level(log::LevelFilter::Info);
        Ok(())
    } else {
        Err(LogError::NoConnection)
    }
}

pub fn init_wait() -> Result<(), log::SetLoggerError> {
    loop {
        if let Ok(backing) = XousLoggerBacking::new() {
            unsafe {
                XOUS_LOGGER_BACKING = Some(backing);
                break;
            }
        }
        xous::yield_slice();
    }
    log::set_logger(&XOUS_LOGGER)?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

pub fn resume() {
    unsafe { XOUS_LOGGER_BACKING.as_mut().unwrap().resume() };
}
