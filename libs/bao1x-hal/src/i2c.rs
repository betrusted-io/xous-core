use core::sync::atomic::{AtomicU32, Ordering};

use bao1x_api::*;
use xous_ipc::Buffer;

pub struct I2c {
    conn: xous::CID,
}

impl I2c {
    pub fn new() -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_api_names::XousNames::new().unwrap();
        let conn = xns
            .request_connection(bao1x_api::SERVER_NAME_BAO1X_HAL)
            .expect("Couldn't connect to bao1x HAL server");
        I2c { conn }
    }

    /// This is used to pass a list of I2C transactions that must be completed atomically
    /// No further I2C requests may happen while this is processing.
    pub fn i2c_transactions(&self, list: I2cTransactions) -> Result<I2cTransactions, xous::Error> {
        let mut buf = Buffer::into_buf(list).map_err(|_| xous::Error::InternalError)?;
        buf.lend_mut(self.conn, HalOpcode::I2c as u32)?;
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
    ) -> Result<I2cResult, xous::Error> {
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
        if result.transactions[0].result == I2cResult::InternalError {
            Err(xous::Error::InternalError)
        } else {
            buf.copy_from_slice(&result.transactions[0].data);
            Ok(result.transactions[0].result)
        }
    }

    fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<I2cResult, xous::Error> {
        let w = I2cTransaction {
            i2c_type: I2cTransactionType::Write,
            device: dev,
            address: adr,
            data: data.to_vec(),
            result: I2cResult::Pending,
        };
        let result = self.i2c_transactions(I2cTransactions::from(vec![w]))?;
        if result.transactions[0].result == I2cResult::InternalError {
            Err(xous::Error::InternalError)
        } else {
            Ok(result.transactions[0].result)
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
