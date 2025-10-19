pub mod bao1x;
pub use bao1x::*;
pub mod debug;
pub mod gfx;
pub mod irq;
pub mod slots;
pub mod usb;

pub const UART_BAUD: u32 = 1_000_000;
