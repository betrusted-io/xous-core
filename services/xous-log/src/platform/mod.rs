#[cfg(any(feature="precursor", feature="renode"))]
#[macro_use]
pub mod precursor;

#[cfg(any(feature="precursor", feature="renode"))]
pub use precursor::debug;

#[cfg(not(target_os = "xous"))]
#[macro_use]
pub mod hosted;

#[cfg(not(target_os = "xous"))]
pub use hosted::implementation;
