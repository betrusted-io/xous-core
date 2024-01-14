// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

/// This is the last byte on all commands
#[derive(Clone, Copy, Eq, PartialEq, Debug, Default, Packed)]
#[packed(big_endian, lsb0)]
pub struct Control {
    #[pkd(7, 6, 0, 0)]
    pub vendor_specific: u8,

    #[pkd(2, 2, 0, 0)]
    pub normal_aca: bool,
}