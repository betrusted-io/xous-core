use core::sync::atomic::{AtomicU32, Ordering};
use num_traits::*;
use xous::{send_message, Message};

pub const SERVER_NAME_ES: &str = "_EARLY_SETTINGS";

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum Opcode {
    /// Sets the keymap in early settings.
    SetKeymap,

    /// Retrieves keymap from early settings.
    GetKeymap,

    /// Retrieves the status of the early sleep flag.
    EarlySleep,

    /// Sets early sleep flag in early settings.
    SetEarlySleep,
}

#[doc = include_str!("../README.md")]
#[derive(Debug)]
pub struct EarlySettings {
    conn: xous::CID,
}

impl EarlySettings {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns
            .request_connection_blocking(SERVER_NAME_ES)
            .expect("Can't connect to EarlySettings");
        Ok(EarlySettings { conn })
    }

    /// Sets map as keymap in the early settings FLASH section.
    /// No validation on map is done, use with caution.
    pub fn set_keymap(&self, map: usize) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::SetKeymap.to_usize().unwrap(),
                map.into(),
                0,
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    /// Gets the keymap from the early settings FLASH section.
    /// No validation is done on the return value, use with caution.
    pub fn get_keymap(&self) -> Result<usize, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::GetKeymap.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(code)) => Ok(code),
            _ => Err(xous::Error::InternalError),
        }
    }

    /// Sets value in the early settings FLASH section.
    pub fn set_early_sleep(&self, value: bool) -> Result<(), xous::Error> {
        send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::SetEarlySleep.to_usize().unwrap(),
                value as usize,
                0,
                0,
                0,
            ),
        )
        .map(|_| ())
    }

    /// Retrieves the early sleep flag from the early settings FLASH section.
    pub fn early_sleep(&self) -> Result<bool, xous::Error> {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(Opcode::EarlySleep.to_usize().unwrap(), 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar1(value)) => match value {
                0 => Ok(false),
                1 => Ok(true),
                _ => Ok(false), // instead of doing `value != 0` we're explicitly matching against specific values, because
                                // reading off FLASH can yield a true value otherwise, even if it wasn't set before
            },
            _ => Err(xous::Error::InternalError),
        }
    }
}

static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for EarlySettings {
    fn drop(&mut self) {
        // now de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
