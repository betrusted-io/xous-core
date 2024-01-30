// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use core::convert::Infallible;

/// Enum of possible errors returned from packing functions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Pack or Unpack method called with a slice of insufficient length
    /// Check the `PACK_BYTES_LEN` on the struct impl
    InsufficientBytes,

    /// Attempted to unpack an enum but the value found didn't match any
    /// known discriminants
    InvalidEnumDiscriminant,

    /// Can't actually be constructed as Infallible can never actually exist
    Infallible(Infallible),
}

impl From<Infallible> for Error {
    fn from(i: Infallible) -> Error {
        Error::Infallible(i)
    }
}