// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use crate::{
    Unsigned, U0, U1, U2, U3, U4, U5, U6, U7, U8,
    IsLess,
};

/// Trait signifying a single bit in a byte. 7 = most significant bit, 0 = least significant bit
pub trait Bit: IsLess<U8> + Unsigned {
    /// The mask used to discard bits before this bit (i.e. if this bit is 5, ANDing this mask with
    /// a u8 will ensure bits 7 and 6 are 0.
    const HEAD_MASK: u8 = ((1_u16 << (Self::USIZE + 1)) - 1) as u8;
    /// The mask used to extract the single bit from a byte
    const BIT_MASK: u8 = 1 << Self::USIZE;
    /// The mask used to discard bits after this bit (i.e. if this bit is 5, ANDing this mask with
    /// a u8 will ensure bits 4 to 0 are 0
    const TAIL_MASK: u8 = !((1_u16 << Self::USIZE) - 1) as u8;
}

impl Bit for U0 { }
impl Bit for U1 { }
impl Bit for U2 { }
impl Bit for U3 { }
impl Bit for U4 { }
impl Bit for U5 { }
impl Bit for U6 { }
impl Bit for U7 { }