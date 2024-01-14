use core::sync::atomic::{AtomicU32, Ordering};

use num_traits::*;
use xous::CID;
use xous_ipc::Buffer;

use crate::api::*;

// these exist outside the I2C struct because it needs to synchronize across multiple object instances within
// the same process
static REFCOUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct I2c {
    conn: CID,
    timeout_ms: u32,
}
impl I2c {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(SERVER_NAME_I2C).expect("Can't connect to I2C");
        I2c { conn, timeout_ms: 150 }
    }

    /// Safety: caller must ensure that there are no I2C actions in flight. This resets the mutex to not
    /// acquired.
    pub unsafe fn i2c_driver_reset(&mut self) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(I2cOpcode::I2cDriverReset.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("error handling i2c driver reset");
    }

    pub fn i2c_set_timeout(&mut self, timeout: u32) { self.timeout_ms = timeout; }

    /// Blocks if another I2C operation is in progress, and resumes once the mutex is released
    /// This *must* be called before doing any I2C transaction -- even if a single action. This
    /// operation exists because multiple I2C operations may have to be executed in sequence without
    /// another thread interrupting the operation for the result to be valid. However, the API
    /// only speaks I2C transactions at a single register read/write level, and can't know if a
    /// subsequent read or write depends on the current state. Thus, we must acquire this mutex.
    /// The mutex sharing is collaborative, so someone without the mutex could, in theory, just
    /// barge in and perform an operation.
    pub fn i2c_mutex_acquire(&self) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(I2cOpcode::I2cMutexAcquire.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("error handling i2c mutex acquire");
    }

    /// Must be called after acquire to release the mutex, otherwise the block retains a hold on the system.
    pub fn i2c_mutex_release(&self) {
        xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(I2cOpcode::I2cMutexRelease.to_usize().unwrap(), 0, 0, 0, 0),
        )
        .expect("error handling i2c mutex release");
    }

    /// initiate an i2c write. This is always a blocking call. In practice, it turns out it's not terribly
    /// useful to just "fire and forget" i2c writes, because actually we cared about the side effect of the
    /// write and don't want execution to move on until the write has been committed,
    /// even if the write "takes a long time"
    pub fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<I2cStatus, xous::Error> {
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory);
        }
        let mut transaction = I2cTransaction::new();

        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        for i in 0..data.len() {
            txbuf[i + 1] = data[i];
        }
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = (data.len() + 1) as u32;
        transaction.timeout_ms = self.timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cResult, _>().unwrap();
        match result.status {
            I2cStatus::ResponseWriteOk => Ok(I2cStatus::ResponseWriteOk),
            _ => {
                log::error!("I2C error: {:?}", result);
                Err(xous::Error::InternalError)
            }
        }
    }

    /// initiate an i2c read. always blocks until done. uses a "repeated start" to switch between
    /// addressing and reading the device.
    pub fn i2c_read(&mut self, dev: u8, adr: u8, data: &mut [u8]) -> Result<I2cStatus, xous::Error> {
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory);
        }
        let mut transaction = I2cTransaction::new();
        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        let rxbuf = [0; I2C_MAX_LEN];
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = 1;
        transaction.rxbuf = Some(rxbuf);
        transaction.rxlen = data.len() as u32;
        transaction.timeout_ms = self.timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cResult, _>().unwrap();
        match result.status {
            I2cStatus::ResponseReadOk => {
                for (&src, dst) in result.rxbuf[..result.rxlen as usize].iter().zip(data.iter_mut()) {
                    *dst = src;
                }
                Ok(I2cStatus::ResponseReadOk)
            }
            _ => {
                log::error!("I2C error: {:?}", result);
                Err(xous::Error::InternalError)
            }
        }
    }

    /// initiate an i2c read, but for devices that don't support repeated starts, such as the AB-RTCMC-32.768
    pub fn i2c_read_no_repeated_start(
        &mut self,
        dev: u8,
        adr: u8,
        data: &mut [u8],
    ) -> Result<I2cStatus, xous::Error> {
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory);
        }
        let mut transaction = I2cTransaction::new();
        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        let rxbuf = [0; I2C_MAX_LEN];
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = 1;
        transaction.rxbuf = Some(rxbuf);
        transaction.rxlen = data.len() as u32;
        transaction.timeout_ms = self.timeout_ms;
        transaction.use_repeated_start = false;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cResult, _>().unwrap();
        match result.status {
            I2cStatus::ResponseReadOk => {
                for (&src, dst) in result.rxbuf[..result.rxlen as usize].iter().zip(data.iter_mut()) {
                    *dst = src;
                }
                Ok(I2cStatus::ResponseReadOk)
            }
            _ => {
                log::error!("I2C error: {:?}", result);
                Err(xous::Error::InternalError)
            }
        }
    }
}

impl Drop for I2c {
    fn drop(&mut self) {
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).ok();
            }
        }
    }
}
