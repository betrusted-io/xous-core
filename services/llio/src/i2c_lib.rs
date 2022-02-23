use xous::{CID, Message};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use crate::api::*;
use std::sync::{Arc, Mutex};
use std::thread;

// these exist outside the I2C struct because it needs to synchronize across multiple object instances within the same process
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
static I2C_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub struct I2c {
    conn: CID,
    cb_sid: xous::SID,
    timeout_ms: u32,
    rx_handoff: Arc<Mutex<Vec<u8>>>,
    rx_hook: Arc<Mutex<Option<I2cAsyncReadHook>>>,
}
impl I2c {
    pub fn new(xns: &xous_names::XousNames) -> Self {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn = xns.request_connection_blocking(SERVER_NAME_I2C).expect("Can't connect to I2C");
        let cb_sid = xous::create_server().unwrap();

        let rx_hook = Arc::new(Mutex::new(None::<I2cAsyncReadHook>));
        let rx_handoff = Arc::new(Mutex::new(Vec::<u8>::new()));

        let _ = thread::spawn({
            let sid = cb_sid.clone();
            let hook = rx_hook.clone();
            let handoff = rx_handoff.clone();
            move || {
                loop {
                    let msg = xous::receive_message(sid).unwrap();
                    match FromPrimitive::from_usize(msg.body.id()) {
                        Some(I2cCallback::Result) => {
                            let buffer = unsafe { Buffer::from_memory_message(msg.body.memory_message().unwrap()) };
                            let trans = buffer.to_original::<I2cTransaction, _>().unwrap();
                            if trans.status == I2cStatus::ResponseReadOk {
                                // grab the mutex on the rx_handoff buff for this block
                                let mut rx_handoff = handoff.lock().unwrap();
                                rx_handoff.clear();
                                // stash a copy of the received value - to be picked up by the blocking call, if specified
                                if let Some(rxbuf) = trans.rxbuf {
                                    for &rx in rxbuf[..trans.rxlen as usize].iter() {
                                        rx_handoff.push(rx);
                                    }
                                }
                                // send a message to the hook, if specified
                                if let Some(&read_hook) = hook.lock().unwrap().as_ref() {
                                    let mut result = I2cReadResult {
                                        rxbuf: [0; 33],
                                        rxlen: 0,
                                        status: I2cStatus::ResponseReadOk,
                                    };
                                    if let Some(rxbuf) = trans.rxbuf {
                                        for (&src, dst) in rxbuf[..trans.rxlen as usize].iter().zip(result.rxbuf.iter_mut()) {
                                            *dst = src;
                                        }
                                        result.rxlen = trans.rxlen;
                                    }
                                    let buf = Buffer::into_buf(result).unwrap();
                                    buf.send(read_hook.conn, read_hook.id).expect("couldn't send callback result");
                                }
                                I2C_BUSY.store(false, Ordering::SeqCst);
                            }
                            if trans.status == I2cStatus::ResponseWriteOk {
                                I2C_BUSY.store(false, Ordering::SeqCst);
                            }
                        }
                        Some(I2cCallback::Drop) => {
                            xous::return_scalar(msg.sender, 1).unwrap(); // acknowledge the drop
                            break;
                        }
                        _ => {
                            log::warn!("received unknown message: {:?}", msg);
                        }
                    }
                }
                xous::destroy_server(sid).unwrap();
            }
        });
        I2c {
            conn,
            cb_sid,
            timeout_ms: 10,
            rx_handoff,
            rx_hook,
        }
    }

    pub fn i2c_set_timeout(&mut self, timeout: u32) {
        self.timeout_ms = timeout;
    }

    /// initiate an i2c write. This is always a blocking call. In practice, it turns out it's not terribly
    /// useful to just "fire and forget" i2c writes, because actually we cared about the side effect of the
    /// write and don't want execution to move on until the write has been committed,
    /// even if the write "takes a long time"
    pub fn i2c_write(&mut self, dev: u8, adr: u8, data: &[u8]) -> Result<I2cStatus, xous::Error> {
        match I2C_BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) { // if the interface is busy, reject the call
            Err(_) => return Err(xous::Error::ServerQueueFull),
            Ok(_) => (), // continue on
        }
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory)
        }
        let mut transaction = I2cTransaction::new();

        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        for i in 0..data.len() {
            txbuf[i+1] = data[i];
        }
        transaction.listener = Some(self.cb_sid.to_u32());
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = (data.len() + 1) as u32;
        transaction.status = I2cStatus::RequestIncoming;
        transaction.timeout_ms = self.timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cStatus, _>().unwrap();
        if result != I2cStatus::ResponseInProgress {
            return Err(xous::Error::OutOfMemory); // indicates that the I2C work queue was full and the request was denied
        }
        while I2C_BUSY.load(Ordering::Relaxed) {
            xous::yield_slice();
        }
        Ok(I2cStatus::ResponseWriteOk)
    }

    /// initiate an i2c read. if asyncread_cb is `None`, one will be provided and the routine will synchronously block until read is complete.
    /// synchronous reads will return the data in &mut `data`. Asynchronous reads will provide the result in the `rxbuf` field of the `I2cTransaction`
    /// returned via the callback. Note that the callback API may be revised to return a smaller, more targeted structure in the future.
    pub fn i2c_read(&mut self, dev: u8, adr: u8, data: &mut [u8], asyncread_cb: Option<I2cAsyncReadHook>) -> Result<I2cStatus, xous::Error> {
        match I2C_BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) { // if the interface is busy, reject the call
            Err(_) => return Err(xous::Error::ServerQueueFull),
            Ok(_) => (), // continue on
        }
        if data.len() > I2C_MAX_LEN - 1 {
            return Err(xous::Error::OutOfMemory)
        }
        let mut transaction = I2cTransaction::new();
        transaction.listener = Some(self.cb_sid.to_u32());
        let mut txbuf = [0; I2C_MAX_LEN];
        txbuf[0] = adr;
        let rxbuf = [0; I2C_MAX_LEN];
        transaction.bus_addr = dev;
        transaction.txbuf = Some(txbuf);
        transaction.txlen = 1;
        transaction.rxbuf = Some(rxbuf);
        transaction.rxlen = data.len() as u32;
        transaction.status = I2cStatus::RequestIncoming;
        transaction.timeout_ms = self.timeout_ms;

        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;
        let result = buf.to_original::<I2cStatus, _>().unwrap();
        if let Some(hook) = asyncread_cb {
            *self.rx_hook.lock().unwrap() = Some(hook);
            Ok(result)
        } else {
            if result != I2cStatus::ResponseInProgress {
                return Err(xous::Error::OutOfMemory);
            }
            while I2C_BUSY.load(Ordering::Relaxed) {
                xous::yield_slice();
            }
            for (&src, dst) in self.rx_handoff.lock().unwrap().iter().zip(data.iter_mut()) {
                *dst = src;
            }
            Ok(I2cStatus::ResponseReadOk)
        }
    }
}

fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid,
        Message::new_blocking_scalar(I2cCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(cid).ok();}
}
impl Drop for I2c {
    fn drop(&mut self) {
        drop_conn(self.cb_sid);
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe{xous::disconnect(self.conn).ok();}
        }
    }
}
