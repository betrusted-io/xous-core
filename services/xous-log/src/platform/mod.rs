#[cfg(any(feature="precursor", feature="renode"))]
#[macro_use]
pub mod precursor;

#[cfg(any(feature="precursor", feature="renode"))]
pub use precursor::debug;

#[cfg(any(feature="precursor", feature="renode"))]
pub use precursor::implementation;

#[cfg(not(target_os = "xous"))]
#[macro_use]
pub mod hosted;

#[cfg(not(target_os = "xous"))]
pub use hosted::implementation;

#[cfg(any(feature="atsama5d27"))]
#[macro_use]
pub mod atsama5d2;

#[cfg(feature="atsama5d27")]
pub use atsama5d2::debug;

#[cfg(feature="atsama5d27")]
pub use atsama5d2::console;

#[cfg(feature="atsama5d27")]
pub use atsama5d2::implementation;
