#[cfg(target_os = "none")]
pub mod native;
#[cfg(target_os = "none")]
pub use native::*;

#[cfg(not(target_os = "none"))]
pub mod hosted;
#[cfg(not(target_os = "none"))]
pub use hosted::*;
