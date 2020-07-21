#[cfg(not(any(windows,unix)))]
pub mod native;
#[cfg(not(any(windows,unix)))]
pub use native::*;

#[cfg(any(windows,unix))]
pub mod hosted;
#[cfg(any(windows,unix))]
pub use hosted::*;
