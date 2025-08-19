pub mod cramium;
pub use cramium::*;
#[cfg(feature = "nto-bio")]
pub mod bio;
pub mod debug;
pub mod irq;

pub const UART_BAUD: u32 = 1_000_000;
