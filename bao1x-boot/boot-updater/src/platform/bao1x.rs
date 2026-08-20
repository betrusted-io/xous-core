pub mod bao1x;
pub use bao1x::*;
pub mod debug;
pub mod irq;

pub const UART_BAUD: u32 = bao1x_api::UART_BAUD;
