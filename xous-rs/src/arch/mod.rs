#[cfg(any(target_os = "none", target_os = "xous"))]
pub mod riscv;
#[cfg(any(target_os = "none", target_os = "xous"))]
pub use riscv::*;

#[cfg(all(any(windows,unix), not(feature = "processes-as-threads")))]
pub mod hosted;
#[cfg(all(any(windows,unix), not(feature = "processes-as-threads")))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
