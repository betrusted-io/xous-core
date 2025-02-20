use core::sync::atomic::{AtomicU32, Ordering};

use cramium_api::*;
use xous_ipc::Buffer;

use crate::api::Opcode;

pub struct I2c {
    conn: xous::CID,
}

impl I2c {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_names::XousNames::new().unwrap();
        let conn = xns
            .request_connection(crate::SERVER_NAME_CRAM_HAL)
            .expect("Couldn't connect to Cramium HAL server");
        I2c { conn }
    }

    /// This is used to pass a list of I2C transactions that must be completed atomically
    /// No further I2C requests may happen while this is processing.
    pub fn i2c_transactions(&self, list: I2cTransactions) -> Result<I2cTransactions, xous::Error> {
        let mut buf = Buffer::into_buf(list).map_err(|_| xous::Error::InternalError)?;
        buf.lend_mut(self.conn, Opcode::I2c as u32)?;
        buf.to_original()
    }
}

impl I2cApi for I2c {
    fn i2c_read(
        &mut self,
        dev: u8,
        adr: u8,
        buf: &mut [u8],
        repeated_start: bool,
    ) -> Result<usize, xous::Error> {
        let r = I2cTransaction {
            i2c_type: if repeated_start {
                I2cTransactionType::ReadRepeatedStart
            } else {
                I2cTransactionType::Read
            },
            device: dev,
            address: adr,
            data: buf.to_vec(),
            result: I2cResult::Pending,
        };
        let result = self.i2c_transactions(I2cTransactions::from(vec![r]))?;
        match result.transactions[0].result {
            I2cResult::Ack(b) => {
                buf.copy_from_slice(&result.transactions[0].data);
                Ok(b)
            }
            _ => Err(xous::Error::InternalError),
        }
    }

    fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<usize, xous::Error> {
        let w = I2cTransaction {
            i2c_type: I2cTransactionType::Write,
            device: dev,
            address: adr,
            data: data.to_vec(),
            result: I2cResult::Pending,
        };
        let result = self.i2c_transactions(I2cTransactions::from(vec![w]))?;
        match result.transactions[0].result {
            I2cResult::Ack(b) => Ok(b),
            _ => Err(xous::Error::InternalError),
        }
    }
}

static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for I2c {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
