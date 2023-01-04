#![no_std]

mod bulk_only_transport;

pub use bulk_only_transport::{
    BulkOnlyTransport,
    CommandBlockWrapper,
    TransferState,
    Error,
};

mod logging {
    pub use itm_logger::*;

    #[cfg(feature = "trace-bot-headers")]
    pub use itm_logger::trace as trace_bot_headers;

    #[cfg(feature = "trace-bot-states")]
    pub use itm_logger::trace as trace_bot_states;

    #[cfg(feature = "trace-bot-bytes")]
    pub use itm_logger::trace as trace_bot_bytes;

    #[cfg(feature = "trace-bot-zlp")]
    pub use itm_logger::trace as trace_bot_zlp;

    #[cfg(feature = "trace-bot-buffer")]
    pub use itm_logger::trace as trace_bot_buffer;
    
    #[cfg(feature = "trace-usb-control")]
    pub use itm_logger::trace as trace_usb_control;

    pub use log::info as trace_bot_headers;
    pub use log::info as trace_bot_states;
    pub use log::info as trace_bot_bytes;
    pub use log::info as trace_bot_zlp;
    pub use log::info as trace_bot_buffer;
    pub use log::info as trace_usb_control;
}
