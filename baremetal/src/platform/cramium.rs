pub mod cramium;
pub use cramium::*;
#[cfg(feature = "nto-bio")]
pub mod bio;
pub mod debug;
pub mod irq;
#[cfg(feature = "nto-usb")]
pub mod usb;

pub const UART_BAUD: u32 = 1_000_000;
