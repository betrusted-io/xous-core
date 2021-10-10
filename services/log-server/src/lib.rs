#![cfg_attr(target_os = "none", no_std)]
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};
use num_traits::ToPrimitive;
use xous_ipc::Buffer;

pub mod api;
mod cursor;

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

const BUFFER_SIZE: usize = 4096;
static mut XOUS_LOGGER_BACKING: Option<XousLoggerBacking> = None;

struct XousLoggerBacking<'a> {
    conn: xous::CID,
    buffer: Buffer<'a>,
}

impl<'a> XousLoggerBacking<'a> {
    pub fn new() -> Result<Self, xous::Error> {
        Ok(XousLoggerBacking {
            conn: xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap())?,
            buffer: Buffer::new(BUFFER_SIZE),
        })
    }
}

impl Default for XousLoggerBacking<'_> {
    fn default() -> Self {
        XousLoggerBacking {
            conn: xous::connect(xous::SID::from_bytes(b"xous-log-server ").unwrap()).unwrap(),
            buffer: Buffer::new(BUFFER_SIZE),
        }
    }
}

impl XousLoggerBacking<'_> {
    fn log_impl(&mut self, record: &log::Record) {
        {
            assert!(core::mem::size_of::<api::LogRecord>() < BUFFER_SIZE);
            let log_record = unsafe { &mut *(self.buffer.as_mut_ptr() as *mut api::LogRecord) };

            log_record.line = record.line();
            log_record.level = record.level() as u32;

            let file = record.file().unwrap_or_default().as_bytes();
            log_record.file_length = file.len() as u32;
            for (dest, src) in log_record.file.iter_mut().zip(file) {
                *dest = *src;
            }

            let module = record.module_path().unwrap_or_default().as_bytes();
            log_record.module_length = module.len() as u32;
            for (dest, src) in log_record.module.iter_mut().zip(module) {
                *dest = *src;
            }

            let mut wrapper = cursor::BufferWrapper::new(&mut log_record.args);
            write!(wrapper, "{}", record.args()).ok(); // truncate if error
            log_record.args_length = wrapper.len() as u32;
        }

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
