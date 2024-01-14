// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

// There are many more variants (see asc-num.txt) but these are the ones the scsi code
// currently uses
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdditionalSenseCode {
    /// ASC 0x20, ASCQ: 0x0 - INVALID COMMAND OPERATION CODE
    InvalidCommandOperationCode,
    /// ASC 0x64, ASCQ: 0x1 - INVALID PACKET SIZE
    InvalidPacketSize,
    /// ASC 0x24, ASCQ: 0x0 - INVALID FIELD IN CDB
    InvalidFieldInCdb,
    /// ASC 0x0, ASCQ: 0x0 - NO ADDITIONAL SENSE INFORMATION
    NoAdditionalSenseInformation,
    /// ASC 0xC, ASCQ: 0x0 - WRITE ERROR
    WriteError,
    /// ASC 0x51, ASCQ: 0x0 - ERASE FAILURE
    EraseFailure,
    /// ASC 0x21, ASCQ: 0x0 - LOGICAL BLOCK ADDRESS OUT OF RANGE
    LogicalBlockAddressOutOfRange,
}

impl AdditionalSenseCode {
    /// Returns the ASC code for this variant
    pub fn asc(&self) -> u8 {
        match self {
            AdditionalSenseCode::InvalidCommandOperationCode => 32,
            AdditionalSenseCode::InvalidPacketSize => 100,
            AdditionalSenseCode::InvalidFieldInCdb => 36,
            AdditionalSenseCode::NoAdditionalSenseInformation => 0,
            AdditionalSenseCode::WriteError => 12,
            AdditionalSenseCode::EraseFailure => 81,
            AdditionalSenseCode::LogicalBlockAddressOutOfRange => 33,
        }
    }
    /// Returns the ASCQ code for this variant
    pub fn ascq(&self) -> u8 {
        match self {
            AdditionalSenseCode::InvalidCommandOperationCode => 0,
            AdditionalSenseCode::InvalidPacketSize => 1,
            AdditionalSenseCode::InvalidFieldInCdb => 0,
            AdditionalSenseCode::NoAdditionalSenseInformation => 0,
            AdditionalSenseCode::WriteError => 0,
            AdditionalSenseCode::EraseFailure => 0,
            AdditionalSenseCode::LogicalBlockAddressOutOfRange => 0,
        }
    }
    /// Returns the ASCQ code for this variant
    pub fn from(asc: u8, ascq: u8) -> core::option::Option<Self> {
        match (asc, ascq) {
            (32, 0) => Some(AdditionalSenseCode::InvalidCommandOperationCode),
            (100, 1) => Some(AdditionalSenseCode::InvalidPacketSize),
            (36, 0) => Some(AdditionalSenseCode::InvalidFieldInCdb),
            (0, 0) => Some(AdditionalSenseCode::NoAdditionalSenseInformation),
            (12, 0) => Some(AdditionalSenseCode::WriteError),
            (81, 0) => Some(AdditionalSenseCode::EraseFailure),
            (33, 0) => Some(AdditionalSenseCode::LogicalBlockAddressOutOfRange),
            _ => None,
        }
    }
}

impl packing::PackedSize for AdditionalSenseCode {
    const BYTES: usize = 2;
}

impl packing::PackedBytes<[u8; 2]> for AdditionalSenseCode {
    type Error = packing::Error;

    fn to_bytes<En: packing::Endian>(&self) -> Result<[u8; 2], Self::Error> {
        Ok([self.asc(), self.ascq()])
    }

    fn from_bytes<En: packing::Endian>(bytes: [u8; 2]) -> Result<Self, Self::Error> {
        let [asc, ascq] = bytes;
        Self::from(asc, ascq).ok_or(packing::Error::InvalidEnumDiscriminant)
    }
}


impl Default for AdditionalSenseCode {
    fn default() -> Self {
        AdditionalSenseCode::NoAdditionalSenseInformation
    }
}