// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

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