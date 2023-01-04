use packing::Error as PackingError;
use usbd_bulk_only_transport::Error as BulkOnlyTransportError;
use usb_device::UsbError;
use crate::block_device::BlockDeviceError;

#[derive(Debug)]
pub enum Error {
    UnhandledOpCode,
    /// The identified opcode requires more data than was sent
    InsufficientDataForCommand,
    PackingError(PackingError),
    BlockDeviceError(BlockDeviceError),
    BulkOnlyTransportError(BulkOnlyTransportError),
}

impl From<PackingError> for Error {
    fn from(e: PackingError) -> Error {
        Error::PackingError(e)
    }
}

impl From<BlockDeviceError> for Error {
    fn from(e: BlockDeviceError) -> Error {
        Error::BlockDeviceError(e)
    }
}

impl From<BulkOnlyTransportError> for Error {
    fn from(e: BulkOnlyTransportError) -> Error {
        Error::BulkOnlyTransportError(e)
    }
}

impl From<UsbError> for Error {
    fn from(e: UsbError) -> Error {
        Error::BulkOnlyTransportError(e.into())
    }
}