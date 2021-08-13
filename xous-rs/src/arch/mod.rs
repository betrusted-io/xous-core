#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod native;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use native::*;

#[cfg(all(any(windows,unix), not(test)))]
pub mod hosted;
#[cfg(all(any(windows,unix), not(test)))]
pub use hosted::*;

#[cfg(all(any(windows,unix), test))]
pub mod test;
#[cfg(all(any(windows,unix), test))]
pub use test::*;