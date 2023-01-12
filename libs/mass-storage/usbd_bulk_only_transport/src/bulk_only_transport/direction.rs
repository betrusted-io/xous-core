// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

/// The direction of a data transfer
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum Direction {
    /// Host to device, OUT in USB parlance
    HostToDevice = 0x00,
    /// Device to host, IN in USB parlance
    DeviceToHost = 0x80,
}