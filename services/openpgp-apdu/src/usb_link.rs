//! IPC to xous-core `usb-bao1x` CCID transport.

use rkyv::{Archive, Deserialize, Serialize};
use xous::CID;
use xous_ipc::Buffer;

pub const SERVER_NAME_USB_DEVICE: &str = "_Xous USB device driver_";

pub const OP_LINK_STATUS: u32 = 0;
pub const OP_CCID_RX_DEFERRED: u32 = 640;
pub const OP_CCID_TX: u32 = 642;

#[derive(Debug, Archive, Serialize, Deserialize, Clone)]
pub struct CcidMsgIpc {
    pub data: Vec<u8>,
    pub code: CcidCode,
}

#[derive(Debug, Archive, Serialize, Deserialize, Copy, Clone, Eq, PartialEq)]
pub enum CcidCode {
    Tx,
    TxAck,
    RxWait,
    RxAck,
    RxTimeout,
    Hangup,
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsbDeviceState {
    Default = 0,
    Addressed = 1,
    Configured = 2,
    Suspend = 3,
}

impl UsbDeviceState {
    pub fn from_scalar(code: usize) -> Option<Self> {
        match code {
            0 => Some(Self::Default),
            1 => Some(Self::Addressed),
            2 => Some(Self::Configured),
            3 => Some(Self::Suspend),
            _ => None,
        }
    }
}

pub struct CcidLink {
    conn: CID,
}

impl CcidLink {
    pub fn connect_to_usb_driver() -> Result<Self, xous::Error> {
        let xns = xous_names::XousNames::new()?;
        let conn = xns.request_connection_blocking(SERVER_NAME_USB_DEVICE)?;
        Ok(Self { conn })
    }

    pub fn link_status(&self) -> UsbDeviceState {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(OP_LINK_STATUS as usize, 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(_, code, _, _, _)) => {
                UsbDeviceState::from_scalar(code).unwrap_or(UsbDeviceState::Default)
            }
            _ => UsbDeviceState::Default,
        }
    }

    pub fn receive_rx(&self) -> Result<Vec<u8>, xous::Error> {
        let req = CcidMsgIpc { data: Vec::new(), code: CcidCode::RxWait };
        let mut buf = Buffer::into_buf(req).map_err(|_| xous::Error::InternalError)?;
        buf.lend_mut(self.conn, OP_CCID_RX_DEFERRED).map_err(|_| xous::Error::InternalError)?;
        let ack = buf.to_original::<CcidMsgIpc, _>().map_err(|_| xous::Error::InternalError)?;
        match ack.code {
            CcidCode::RxAck => Ok(ack.data),
            CcidCode::Hangup => Err(xous::Error::ProcessTerminated),
            CcidCode::Denied => Err(xous::Error::AccessDenied),
            _ => Err(xous::Error::InternalError),
        }
    }

    pub fn send_tx(&self, data: Vec<u8>) -> Result<(), xous::Error> {
        let req = CcidMsgIpc { data, code: CcidCode::Tx };
        let mut buf = Buffer::into_buf(req).map_err(|_| xous::Error::InternalError)?;
        buf.lend_mut(self.conn, OP_CCID_TX).map_err(|_| xous::Error::InternalError)?;
        let ack = buf.to_original::<CcidMsgIpc, _>().map_err(|_| xous::Error::InternalError)?;
        match ack.code {
            CcidCode::TxAck => Ok(()),
            CcidCode::Hangup => Err(xous::Error::ProcessTerminated),
            CcidCode::Denied => Err(xous::Error::AccessDenied),
            _ => Err(xous::Error::InternalError),
        }
    }
}
