#[cfg(baremetal)]
pub mod native;
#[cfg(baremetal)]
pub use native::*;

#[cfg(any(windows,unix))]
pub mod hosted;
#[cfg(any(windows,unix))]
pub use hosted::*;
