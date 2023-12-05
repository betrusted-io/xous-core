#![cfg_attr(any(target_os = "none", feature = "nostd"), no_std)]
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};
use num_traits::ToPrimitive;

pub mod api;
mod cursor;

#[derive(Debug)]
pub enum LogError {
    LoggerExists,
    NoConnection,
}

struct XousLogger;
static XOUS_LOGGER: XousLogger = XousLogger {};
static XOUS_LOGGER_CONNECTION: AtomicU32 = AtomicU32::new(0);

impl XousLogger {
    fn log_impl(&self, record: &log::Record) {
        let mut log_record = api::LogRecord::default();
        assert_eq!(core::mem::size_of::<api::LogRecord>(), 4096);

        // A "line" of 0 is the same as "None" for our purposes here.
        log_record.line = core::num::NonZeroU32::new(record.line().unwrap_or_default());

        log_record.level = record.level() as u32;

        let file = record.file().unwrap_or_default().as_bytes();
        log_record.file_length = file.len().min(file.len()) as u32;
        for (dest, src) in log_record.file.iter_mut().zip(file) {
            *dest = *src;
        }

        let module = record.module_path().unwrap_or_default().as_bytes();
        log_record.module_length = module.len().min(module.len()) as u32;
        for (dest, src) in log_record.module.iter_mut().zip(module) {
            *dest = *src;
        }

        // Serialize the text to our record buffer
        let mut wrapper = cursor::BufferWrapper::new(&mut log_record.args);
        write!(wrapper, "{}", record.args()).ok(); // truncate if error
        log_record.args_length = wrapper.len().min(log_record.args.len()) as u32;

        let buf = unsafe {
            xous::MemoryRange::new(
                &log_record as *const api::LogRecord as usize,
                core::mem::size_of::<api::LogRecord>(),
            )
            .unwrap()
        };

        xous::send_message(
            XOUS_LOGGER_CONNECTION.load(Ordering::Relaxed),
            xous::Message::new_lend(
                crate::api::Opcode::LogRecord.to_usize().unwrap(),
                buf,
                None,
                None,
            ),
        )
        .unwrap();
    }

    fn resume(&self) {
        xous::send_message(
            XOUS_LOGGER_CONNECTION.load(Ordering::Relaxed),
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
        XOUS_LOGGER.log_impl(record);
    }

    fn flush(&self) {}
}

pub fn init() -> Result<(), LogError> {
    XOUS_LOGGER_CONNECTION.store(
        xous::try_connect(xous::SID::from_bytes(b"xous-log-server ").unwrap())
            .or(Err(LogError::NoConnection))?,
        Ordering::Relaxed,
    );
    log::set_logger(&XOUS_LOGGER).map_err(|_| LogError::LoggerExists)?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

pub fn init_wait() -> Result<(), ()> {
    let sid = xous::SID::from_bytes(b"xous-log-server ").unwrap();
    let cid = xous::connect(sid).or(Err(()))?;
    XOUS_LOGGER_CONNECTION.store(
        cid,
        Ordering::Relaxed,
    );
    log::set_logger(&XOUS_LOGGER).or(Err(()))?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}

pub fn resume() {
    XOUS_LOGGER.resume();
}
