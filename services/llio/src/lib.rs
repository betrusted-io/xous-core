#![cfg_attr(target_os = "none", no_std)]

/// This is the API that other servers use to call the COM. Read this code as if you
/// are calling these functions inside a different process.
pub mod api;
use api::*;
use rkyv::Deserialize;

use xous::{send_message, CID, XousDeserializer};

// used by the LLIO to send a response to other servers
pub fn send_i2c_response(cid: CID, transaction: I2cTransaction) -> Result<(), xous::Error> {
    let op = api::Opcode::I2cResponse(transaction);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&op).expect("LLIO_API: couldn't archive I2cResponse")
    };
    let buf = writer.into_inner();
    buf.lend(cid, pos as u32).expect("LLIO_API: I2cResponse operation failure");

    Ok(())
}
// used by other servers to request an I2C transaction
pub fn send_i2c_request(cid: CID, transaction: I2cTransaction) -> Result<I2cStatus, xous::Error> {
    let op = api::Opcode::I2cTxRx(transaction);
    let mut writer = rkyv::ser::serializers::BufferSerializer::new(xous::XousBuffer::new(4096));
    let pos = {
        use rkyv::ser::Serializer;
        writer.serialize_value(&op).expect("LLIO_API: couldn't archive I2cTxRx")
    };
    let mut buf = writer.into_inner();

    buf.lend_mut(cid, pos as u32).expect("LLIO_API: I2cTxRx operation failure");

    let returned = unsafe { rkyv::archived_value::<api::Opcode>(buf.as_ref(), pos)};
    if let rkyv::Archived::<api::Opcode>::I2cTxRx(result) = returned {
        let transaction = result.deserialize(&mut XousDeserializer).expect("LLIO_API: Can't deserialize result in send_i2c_request");
        Ok(transaction.status())
    } else {
        log::info!("send_i2c_request saw an unhandled return type of {:?}", buf);
        Err(xous::Error::InternalError)
    }
}

pub fn allow_power_off(cid: CID, allow: bool) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::PowerSelf(!allow).into()).map(|_| ())
}

pub fn allow_ec_snoop(cid: CID, allow: bool) -> Result<(), xous::Error> {
    send_message(cid, api::Opcode::EcSnoopAllow(allow).into()).map(|_| ())
}

pub fn adc_vbus(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcVbus.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_vccint(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcVccInt.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_vccaux(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcVccAux.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_vccbram(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcVccBram.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_usb_n(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcUsbN.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_usb_p(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcUsbP.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_temperature(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcTemperature.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_gpio5(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcGpio5.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}
pub fn adc_gpio2(cid: CID) -> Result<u16, xous::Error> {
    let response = send_message(cid, api::Opcode::AdcGpio2.into())?;
    if let xous::Result::Scalar1(val) = response {
        Ok(val as u16)
    } else {
        log::error!("LLIO: unexpected return value: {:#?}", response);
        Err(xous::Error::InternalError)
    }
}