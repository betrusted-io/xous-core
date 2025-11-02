// Dabao comes along for the ride for baosec
#[cfg(any(feature = "board-baosec", feature = "board-dabao"))]
mod baosec;
#[cfg(any(feature = "board-baosec", feature = "board-dabao"))]
pub use baosec::*;

#[cfg(feature = "hosted-baosec")]
mod hosted;
#[cfg(feature = "hosted-baosec")]
pub use hosted::*;
