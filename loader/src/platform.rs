#[cfg(any(feature="precursor",feature="renode"))]
mod precursor;
#[cfg(any(feature="precursor",feature="renode"))]
pub use precursor::*;

#[cfg(any(feature="cramium-soc", feature="cramium-fpga"))]
mod cramium;
#[cfg(any(feature="cramium-soc", feature="cramium-fpga"))]
pub use cramium::*;