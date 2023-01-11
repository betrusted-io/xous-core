// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum PageControl {
    /// Current values
    CurrentValues = 0b00,
    /// Changeable values
    ChangeableValues = 0b01,
    /// Default values
    DefaultValues = 0b10,
    /// Saved values
    SavedValues = 0b11,
}

impl Default for PageControl {
    fn default() -> Self {
        PageControl::CurrentValues
    }
}
