#[cfg(feature = "board-baosec")]
mod baosec;
#[cfg(feature = "board-baosec")]
pub use baosec::*;

#[cfg(feature = "hosted-baosec")]
mod hosted;
#[cfg(feature = "hosted-baosec")]
pub use hosted::*;
