// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceError {
    /// Hardware didn't behave as expected, unrecoverable
    HardwareError,

    /// Error during writing; most likely value read back after write was wrong
    WriteError,

    /// Error during erase; most likely value read back after erase was wrong
    ///
    /// STM32 flash programming app note implies this is possible but doesn't say under what
    /// circumstances. Is the flash knackered if this happens?
    EraseError,

    /// Address is invalid or out of range
    InvalidAddress,
}

pub trait BlockDevice {
    /// The number of bytes per block. This determines the size of the buffer passed
    /// to read/write functions
    const BLOCK_BYTES: usize;

    /// Read the block indicated by `lba` into the provided buffer
    fn read_block(&self, lba: u32, block: &mut [u8]) -> Result<(), BlockDeviceError>;

    /// Write the `block` buffer to the block indicated by `lba`
    fn write_block(&mut self, lba: u32, block: &[u8]) -> Result<(), BlockDeviceError>;

    /// Get the maxium valid lba (logical block address)
    fn max_lba(&self) -> u32;
}