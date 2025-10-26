pub mod bao1x;
pub use bao1x::*;
#[cfg(feature = "bao1x-trng")]
pub mod avtrng;
#[cfg(feature = "bao1x-bio")]
pub mod bio;
#[cfg(feature = "dabao-selftest")]
pub mod dabao_selftest;
pub mod debug;
pub mod irq;
#[cfg(feature = "bao1x-usb")]
pub mod usb;

pub const UART_BAUD: u32 = bao1x_api::UART_BAUD;
