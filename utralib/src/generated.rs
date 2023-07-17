// Feature flags are checked inside build.rs for sanity, so we
// can use them here assuming all targets are mutually exclusive.

#[cfg(feature="precursor-perflib")]
mod precursor_perf;
#[cfg(feature="precursor-perflib")]
pub use precursor_perf::*;

#[cfg(feature="precursor-dvt")]
mod precursor_dvt;
#[cfg(feature="precursor-dvt")]
pub use precursor_dvt::*;

#[cfg(feature="renode")]
mod renode;
#[cfg(feature="renode")]
pub use renode::*;

#[cfg(feature="precursor-pvt")]
mod precursor_pvt;
#[cfg(feature="precursor-pvt")]
pub use precursor_pvt::*;

#[cfg(feature = "atsama5d27")]
mod atsama5d27;
#[cfg(feature = "atsama5d27")]
pub use atsama5d27::*;

#[cfg(feature="cramium-soc")]
mod cramium_soc;
#[cfg(feature="cramium-soc")]
pub use cramium_soc::*;

#[cfg(feature="cramium-fpga")]
mod cramium_fpga;
#[cfg(feature="cramium-fpga")]
pub use cramium_fpga::*;

// Hosted mode includes nothing, as it relies on the abstract host
// architecture for I/O; so this file is empty when the "hosted"
// configuration is selected and there are no corresponding "hosted"
// mode feature flags.