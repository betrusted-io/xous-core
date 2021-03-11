#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod native;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use native::*;

#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub mod hosted;
#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub use hosted::*;
