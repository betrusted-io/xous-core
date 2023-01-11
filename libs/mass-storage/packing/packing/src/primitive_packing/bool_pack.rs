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

impl PackedSize for bool {
    const BYTES: usize = 1;
}

impl PackedBytes<[u8; Self::BYTES]> for bool {
    type Error = Infallible;
    fn to_bytes<En: Endian>(&self) -> Result<[u8; Self::BYTES], Self::Error> {
        Ok(if *self {
           [1]
        } else {
           [0]
        })
    }
    fn from_bytes<En: Endian>(bytes: [u8; Self::BYTES]) -> Result<Self, Self::Error> {
        Ok(bytes[0] == 1)
    }
}