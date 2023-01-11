// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

use packing::Packed;

/// The status of a command
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum CommandStatus {
    /// Ok, command completed successfully
    CommandOk = 0x00,
    /// Error, command failed
    CommandError = 0x01,
    /// Fatal device error, reset required
    PhaseError = 0x02,
}