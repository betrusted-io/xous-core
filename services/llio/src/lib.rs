#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;
use api::*;

use xous::{send_message, CID, Message, msg_scalar_unpack};
use xous_ipc::Buffer;
use num_traits::{ToPrimitive, FromPrimitive};

// this hooks the responce of the I2C bus
static mut I2C_CB: Option<fn(I2cTransaction)> = None;

#[derive(Debug)]
pub struct Llio {
    conn: CID,
    com_sid: Option<xous::SID>,
    i2c_sid: Option<xous::SID>,
    rtc_sid: Option<xous::SID>,
    usb_sid: Option<xous::SID>,
}
impl Llio {
    pub fn new(xns: &xous_names::XousNames) -> Result<Self, xous::Error> {
        let conn = xns.request_connection_blocking(api::SERVER_NAME_LLIO).expect("Can't connect to MyServer");
        Ok(Llio {
          conn,
          com_sid: None,
          i2c_sid: None,
          rtc_sid: None,
          usb_sid: None,
        })
    }
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
            xous::send_message(self.conn,
                Message::new_scalar(Opcode::I2cRegisterCallback.to_usize().unwrap(),
                sid_tuple.0 as usize, sid_tuple.1 as usize, sid_tuple.2 as usize, sid_tuple.3 as usize
            )).unwrap();
        }
        Ok(())
    }
    // used by other servers to request an I2C transaction
    pub fn send_i2c_request(&self, transaction: I2cTransaction) -> Result<I2cStatus, xous::Error> {
        let mut buf = Buffer::into_buf(transaction).or(Err(xous::Error::InternalError))?;
        buf.lend_mut(self.conn, Opcode::I2cTxRx.to_u32().unwrap()).or(Err(xous::Error::InternalError))?;

        let status = buf.to_original::<I2cStatus, _>().unwrap();
        Ok(status)
    }

    pub fn allow_power_off(&self, allow: bool) -> Result<(), xous::Error> {
        let arg = if allow { 1 } else { 0 };
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
}
impl Drop for Llio {
    fn drop(&mut self) {
        if let Some(sid) = self.i2c_sid.take() {
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_blocking_scalar(I2cCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
            xous::destroy_server(sid).unwrap();
        }
        if let Some(sid) = self.usb_sid.take() {
            let cid = xous::connect(sid).unwrap();
            xous::send_message(cid,
                Message::new_blocking_scalar(EventCallback::Drop.to_usize().unwrap(), 0, 0, 0, 0)).unwrap();
            unsafe{xous::disconnect(cid).unwrap();}
            xous::destroy_server(sid).unwrap();
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
                let i2cresult = buffer.to_original::<I2cTransaction, _>().unwrap();
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
}