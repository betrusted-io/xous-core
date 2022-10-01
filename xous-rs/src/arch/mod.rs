#[cfg(any(feature="precursor", feature="renode"))]
pub mod riscv;
#[cfg(any(feature="precursor", feature="renode"))]
pub use riscv::*;

#[cfg(all(any(windows, unix), not(feature = "processes-as-threads")))]
pub mod hosted;
#[cfg(all(any(windows, unix), not(feature = "processes-as-threads")))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
