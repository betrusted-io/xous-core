// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

mod scsi;
pub use scsi::*;

mod block_device;
pub use block_device::*;

mod logging {
    pub use log::debug as trace_scsi_fs;
    pub use log::debug as trace_scsi_command;
}
