#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;
pub use api::*;

use xous::{send_message, CID, Message, msg_scalar_unpack};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

// this hooks the response of the I2C bus
static mut I2C_CB: Option<fn(I2cTransaction)> = None;

#[derive(Debug)]
pub struct Llio {
    conn: CID,
    i2c_conn: CID,
    com_sid: Option<xous::SID>,
    i2c_sid: Option<xous::SID>,
    rtc_sid: Option<xous::SID>,
    usb_sid: Option<xous::SID>,
    gpio_sid: Option<xous::SID>,
}
impl Llio {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_LLIO).expect("Can't connect to LLIO");
        let i2c_conn = xns.request_connection_blocking(api::SERVER_NAME_I2C).expect("Can't connect to I2C");
        Ok(Llio {
          conn,
          com_sid: None,
          i2c_sid: None,
          rtc_sid: None,
          usb_sid: None,
          gpio_sid: None,
          i2c_conn,
        })
    }
    pub fn vibe(&self, pattern: VibePattern) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::Vibe.to_usize().unwrap(), pattern.into(), 0, 0, 0)
        ).map(|_|())
    }



    ///////////////////////// I2C ///////////////
    // used to hook a callback for I2c responses
    pub fn hook_i2c_callback(&mut self, cb: fn(I2cTransaction)) -> Result<(), xous::Error> {
        if unsafe{I2C_CB}.is_some() {
            return Err(xous::Error::MemoryInUse) // can't hook it twice
        }
        unsafe{I2C_CB = Some(cb)};
        if self.i2c_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.i2c_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(i2c_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            // note: we don't register a callback, because we hand our SID directly to the i2c request for a 1:1 message return
        }
        Ok(())
    }
    // used by other servers to request an I2C transaction
    pub fn send_i2c_request(&self, transaction: I2cTransaction) -> Result<I2cStatus, xous::Error> {
        // copy the transaction, and annotate with our private callback listener server address
        // the server is used only for this connection, and shared only to the LLIO server
        // it's sanitized on the callback response. It's not the end of the world if this server address is
        // discovered, just unhygenic.
        let mut local_transaction = transaction;
        match self.i2c_sid {
            None => {
                local_transaction.listener = None;
                log::warn!("Initiating I2C request with no listener. Are you sure?");
            },
            Some(sid) => local_transaction.listener = Some(sid.to_u32()),
        }
        let mut buf = Buffer::into_buf(local_transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.i2c_conn, I2cOpcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        let status = buf.to_original::<I2cStatus, _>().unwrap();
        Ok(status)
    }
    pub fn poll_i2c_busy(&self) -> Result<bool, xous::Error> {
        let response = send_message(self.i2c_conn,
            Message::new_blocking_scalar(I2cOpcode::I2cIsBusy.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            if val != 0 {
                Ok(true)
            } else {
                Ok(false)
            }
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }

    /*
    pub fn i2c_sync_request(&self, transaction: &mut I2cTransaction) -> Result<I2cStatus, xous::Error> {

    }

    pub fn i2c_async_request(&self, transaction: I2cTransaction, cb: fn(I2cTransaction)) -> Result<I2cStatus, xous::Error> {

    }*/
    ///////////////////////// I2C ///////////////



    pub fn allow_power_off(&self, allow: bool) -> Result<(), xous::Error> {
        let arg = if allow { 0 } else { 1 };
        send_message(self.conn,
            Message::new_scalar(Opcode::PowerSelf.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }

    pub fn allow_ec_snoop(&self, allow: bool) -> Result<(), xous::Error> {
        let arg = if allow { 1 } else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::EcSnoopAllow.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }

    pub fn adc_vbus(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcVbus.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_vccint(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccInt.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_vccaux(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccAux.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_vccbram(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcVccBram.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_usb_n(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcUsbN.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_usb_p(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcUsbP.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_temperature(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcTemperature.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_gpio5(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcGpio5.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn adc_gpio2(&self) -> Result<u16, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::AdcGpio2.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar1(val) = response {
            Ok(val as u16)
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    // USB hooks
    pub fn hook_usb_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.usb_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.usb_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(usb_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::EventUsbAttachSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn usb_event_enable(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::EventUsbAttachEnable.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }
    // RTC alarm hooks
    pub fn hook_rtc_alarm_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.rtc_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.rtc_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(rtc_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::EventRtcSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn rtc_alarm_enable(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::EventRtcEnable.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }
    // COM IRQ hooks
    pub fn hook_com_event_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.com_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.com_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(com_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::EventComSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn com_event_enable(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::EventComEnable.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }
    // GPIO IRQ hooks
    pub fn hook_gpio_event_callback(&mut self, id: u32, cid: CID) -> Result<(), xous::Error> {
        if self.gpio_sid.is_none() {
            let sid = xous::create_server().unwrap();
            self.gpio_sid = Some(sid);
            let sid_tuple = sid.to_u32();
            xous::create_thread_4(gpio_cb_server, sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize).unwrap();
            let hookdata = ScalarHook {
                sid: sid_tuple,
                id,
                cid,
            };
            let buf = Buffer::into_buf(hookdata).or(Err(xous::Error::InternalError))?;
            buf.lend(self.conn, Opcode::GpioIntSubscribe.to_u32().unwrap()).map(|_|())
        } else {
            Err(xous::Error::MemoryInUse) // can't hook it twice
        }
    }
    pub fn ec_reset(&self) -> Result<(), xous::Error> {
        send_message(self.conn,
            Message::new_scalar(Opcode::EcReset.to_usize().unwrap(), 0, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn self_destruct(&self, code: usize) -> Result<(), xous::Error> {
        // it's up to the caller to know the code sequence, which is:
        // 0x2718_2818
        // followed by
        // 0x3141_5926
        send_message(self.conn,
            Message::new_scalar(Opcode::SelfDestruct.to_usize().unwrap(), code, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn boost_on(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn,
            Message::new_scalar(Opcode::PowerBoostMode.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn audio_on(&self, ena: bool) -> Result<(), xous::Error> {
        let arg = if ena { 1 } else { 0 };
        send_message(self.conn,
            Message::new_blocking_scalar(Opcode::PowerAudio.to_usize().unwrap(), arg, 0, 0, 0)
        ).map(|_| ())
    }
    pub fn soc_gitrev(&self) -> Result<(u8, u8, u8, u8, u32), xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::InfoGit.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar2(val1, val2) = response {
            Ok(
                (
                    ((val1 >> 24) as u8), // major
                    ((val1 >> 16) as u8), // minor
                    ((val1 >> 8) as u8),  // rev
                    (val1 >> 0) as u8,    // gitextra
                    val2 as u32  // gitrev
                )
            )
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
    pub fn soc_dna(&self) -> Result<u64, xous::Error> {
        let response = send_message(self.conn,
            Message::new_blocking_scalar(Opcode::InfoDna.to_usize().unwrap(), 0, 0, 0, 0))?;
        if let xous::Result::Scalar2(val1, val2) = response {
            Ok(
                (val1 as u64) | ((val2 as u64) << 32)
            )
        } else {
            log::error!("LLIO: unexpected return value: {:#?}", response);
            Err(xous::Error::InternalError)
        }
    }
}


fn drop_conn(sid: xous::SID) {
    let cid = xous::connect(sid).unwrap();
    xous::send_message(cid,
        Message::new_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
    unsafe{xous::disconnect(cid).unwrap();}
}
impl Drop for Llio {
    fn drop(&mut self) {
        if let Some(sid) = self.i2c_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.usb_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.rtc_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.com_sid.take() {
            drop_conn(sid);
        }
        if let Some(sid) = self.gpio_sid.take() {
            drop_conn(sid);
        }
        unsafe{xous::disconnect(self.conn).unwrap();}
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

/// handles callback messages that indicate a USB interrupt has happened, in the library user's process space.
fn usb_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

/// handles callback messages that indicate a RTC interrupt has happened, in the library user's process space.
fn rtc_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

/// handles callback messages that indicate a COM interrupt has happened, in the library user's process space.
fn com_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}

/// handles callback messages that indicate a GPIO interrupt has happened, in the library user's process space.
fn gpio_cb_server(sid0: usize, sid1: usize, sid2: usize, sid3: usize) {
    let sid = xous::SID::from_u32(sid0 as u32, sid1 as u32, sid2 as u32, sid3 as u32);
    loop {
        let msg = xous::receive_message(sid).unwrap();
        match FromPrimitive::from_usize(msg.body.id()) {
            Some(EventCallback::Event) => msg_scalar_unpack!(msg, cid, id, _, _, {
                // directly pass the scalar message onto the CID with the ID memorized in the original hook
                send_message(cid as u32,
                    Message::new_scalar(id, 0, 0, 0, 0)
                ).unwrap();
            }),
            Some(EventCallback::Drop) => {
                break; // this exits the loop and kills the thread
            }
            None => (),
        }
    }
    xous::destroy_server(sid).unwrap();
}