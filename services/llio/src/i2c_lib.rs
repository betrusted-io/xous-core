use xous::{CID, Message};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use crate::api::*;

static I2C_REFCOUNT: AtomicU32 = AtomicU32::new(0);

// this hooks the response of the I2C bus
static mut I2C_CB: Option<fn(I2cTransaction)> = None;

static I2C_IN_PROGRESS_MUTEX: AtomicBool = AtomicBool::new(false);
static mut I2C_RX_HANDOFF: [u8; I2C_MAX_LEN] = [0; I2C_MAX_LEN]; // this is protected by the above mutex

fn sync_i2c_cb(transaction: I2cTransaction) {
    if let Some(rxbuf) = transaction.rxbuf {
        unsafe {
            for i in 0..transaction.rxlen as usize {
                I2C_RX_HANDOFF[i] = rxbuf[i];
            }
        }
    }
    unsafe{I2C_CB = None;} // break-before-make, ensures that the I2C_RX_HANDOFF data can't be overwritten by another callback
    I2C_IN_PROGRESS_MUTEX.store(false, Ordering::Relaxed);
}

#[derive(Debug)]
pub struct I2c {
    i2c_conn: CID,
    i2c_sid: Option<xous::SID>,
    i2c_timeout_ms: u32,
}
impl I2c {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        I2C_REFCOUNT.store(I2C_REFCOUNT.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
        let i2c_conn = xns.request_connection_blocking(SERVER_NAME_I2C).expect("Can't connect to I2C");
        I2c {
            i2c_sid: None,
            i2c_conn,
            i2c_timeout_ms: 10,
        }
    }

    fn check_cb_init(&mut self) {
        if self.i2c_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.i2c_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(i2c_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            // note: we don't register a callback, because we hand our SID directly to the i2c request for a 1:1 message return
        }
    }
    pub fn i2c_set_timeout(&mut self, timeout: u32) {
        self.i2c_timeout_ms = timeout;
    }

    /// initiate an i2c write. if async_cb is `None`, one will be provided and the routine will synchronously block until write is complete.
    /// if you want to "fire and forget" the write and don't care when or how it finishes, simply provide a dummy callback.
    pub fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8], async_cb: Option<fn(I2cTransaction)>) -> Result<I2cStatus, xous::Error> {
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory)
        }
        let synchronous: bool;
        let mut transaction = I2cTransaction::new();
        let cb = if let Some(callback) = async_cb {
            synchronous = false;
            callback
        } else {
            synchronous = true;
            if I2C_IN_PROGRESS_MUTEX.load(Ordering::Relaxed) {
                log::error!("entering a synchronous write routine, but somehow a synchronous operation was already in progress!");
                return Err(xous::Error::InternalError);
            }
            sync_i2c_cb
        };
        self.check_cb_init();
        unsafe {
            if let Some(old_cb) = I2C_CB {
                if old_cb != cb {
                    log::warn!("Multiple outstanding write transactions, with different callbacks. You are probably making an error!");
                }
            }
            I2C_CB = Some(cb);
        }
        if synchronous {
            I2C_IN_PROGRESS_MUTEX.store(true, Ordering::Relaxed);
        }
        match self.i2c_sid {
            Some(sid) => transaction.listener = Some(sid.to_u32()),
            None => log::error!("We requested a local listener, but somehow it's not there!"),
        }
        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        for i in 0..data.len() {
            txbuf[i+1] = data[i];
        }
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = (data.len() + 1) as u32;
        transaction.status = I2cStatus::RequestIncoming;
        transaction.timeout_ms = self.i2c_timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.i2c_conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cStatus, _>().unwrap();
        if synchronous {
            if result != I2cStatus::ResponseInProgress {
                return Err(xous::Error::OutOfMemory);
            }
            while I2C_IN_PROGRESS_MUTEX.load(Ordering::Relaxed) {
                xous::yield_slice();
            }
            Ok(I2cStatus::ResponseWriteOk)
        } else {
            Ok(result)
        }
    }

    /// initiate an i2c read. if asyncread_cb is `None`, one will be provided and the routine will synchronously block until read is complete.
    /// synchronous reads will return the data in &mut `data`. Asynchronous reads will provide the result in the `rxbuf` field of the `I2cTransaction`
    /// returned via the callback. Note that the callback API may be revised to return a smaller, more targeted structure in the future.
    pub fn i2c_read(&mut self, dev: u8, adr: u8, data: &mut[u8], asyncread_cb: Option<fn(I2cTransaction)>) -> Result<I2cStatus, xous::Error> {
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory)
        }
        let synchronous: bool;
        let read_cb = if let Some(cb) = asyncread_cb {
            // user is supplying an async callback
            synchronous = false;
            cb
        } else {
            // user hasn't provided a callback, we'll provide one and return synchronously
            synchronous = true;
            if I2C_IN_PROGRESS_MUTEX.load(Ordering::Relaxed) {
                log::error!("trying a synchronous read, but somehow a synchronous operation was already in progress!");
                return Err(xous::Error::InternalError);
            }
            sync_i2c_cb // supply a default callback
        };
        let mut transaction = I2cTransaction::new();
        self.check_cb_init();
        unsafe {
            if let Some(old_cb) = I2C_CB {
                if old_cb != read_cb {
                    log::warn!("Multiple outstanding read transactions, with different callbacks. Hope you know what you are doing!");
                }
            }
            I2C_CB = Some(read_cb);
        }
        if synchronous {
            I2C_IN_PROGRESS_MUTEX.store(true, Ordering::Relaxed);
        }
        match self.i2c_sid {
            Some(sid) => transaction.listener = Some(sid.to_u32()),
            None => log::error!("We requested a local listener, but somehow it's not there!"),
        }
        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        let rxbuf = [0; I2C_MAX_LEN];
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = 1;
        transaction.rxbuf = Some(rxbuf);
        transaction.rxlen = data.len() as u32;
        transaction.status = I2cStatus::RequestIncoming;
        transaction.timeout_ms = self.i2c_timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.i2c_conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cStatus, _>().unwrap();
        if synchronous {
            if result != I2cStatus::ResponseInProgress {
                return Err(xous::Error::OutOfMemory);
            }
            while I2C_IN_PROGRESS_MUTEX.load(Ordering::Relaxed) {
                xous::yield_slice();
            }
            unsafe {
                for (&src, dst) in I2C_RX_HANDOFF.iter().zip(data.iter_mut()) {
                    *dst = src;
                }
            }
            Ok(I2cStatus::ResponseReadOk)
        } else {
            Ok(result)
        }
    }
    // used by async callback handlers to indicate their completion, allowing e.g. later synchronous operations
    pub fn i2c_async_done(&self) {
        unsafe{I2C_CB = None};
    }
}

fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid,
        Message::new_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(cid).unwrap();}
}
impl Drop for I2c {
    fn drop(&mut self) {
        if let Some(sid) = self.i2c_sid.take() {
            drop_conn(sid);
        }
        I2C_REFCOUNT.store(I2C_REFCOUNT.load(Ordering::Relaxed) - 1, Ordering::Relaxed);
        if I2C_REFCOUNT.load(Ordering::Relaxed) == 0 {
            unsafe{xous::disconnect(self.i2c_conn).unwrap();}
        }
    }
}


/// handles callback messages from I2C server, in the library user's process space.
fn i2c_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(I2cCallback::Result) => {
                let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                let mut i2cresult = buffer.to_original::<I2cTransaction, _>().unwrap();
                i2cresult.listener = None; // don't leak our local server address to the callback
                unsafe {
                    if let Some(cb) = I2C_CB {
                        cb(i2cresult)
                    }
                }
            },
            Some(I2cCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}
