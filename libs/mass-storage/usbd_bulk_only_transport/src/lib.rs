// Vendored from https://github.com/stm32-rs/stm32-usbd tag v0.6.0
// Original copyright (c) 2021 Matti Virkkunen <mvirkkunen@gmail.com>, Vadim Kaushan <admin@disasm.info>,
// Nicolas Stalder <n@stalder.io>", Jonas Martin <lichtfeind@gmail.com>
// SPDX-License-Identifier: MIT
// SPDX-LIcense-Identifier: Apache 2.0

mod bulk_only_transport;

pub use bulk_only_transport::{
    BulkOnlyTransport,
    CommandBlockWrapper,
    TransferState,
    Error,
};

mod logging {
    pub use log::debug as trace_bot_headers;
    pub use log::debug as trace_bot_states;
    pub use log::trace as trace_bot_bytes;
    pub use log::trace as trace_bot_zlp;
    pub use log::trace as trace_bot_buffer;
    pub use log::debug as trace_usb_control;
}
