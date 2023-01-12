// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use crate::{
    Endian,
    PackedBytes,
    PackedSize,
};
use core::convert::Infallible;

impl PackedSize for i32 {
    const BYTES: usize = 4;
}

impl PackedBytes<[u8; Self::BYTES]> for i32 {
    type Error = Infallible;
    fn to_bytes<En: Endian>(&self) -> Result<[u8; Self::BYTES], Self::Error> {
        Ok(if En::IS_LITTLE {
            self.to_le_bytes()
        } else {
            self.to_be_bytes()
        })
    }
    fn from_bytes<En: Endian>(bytes: [u8; Self::BYTES]) -> Result<Self, Self::Error> {
        Ok(if En::IS_LITTLE {
            Self::from_le_bytes(bytes)
        } else {
            Self::from_be_bytes(bytes)
        })
    }
}
