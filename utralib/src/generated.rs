// Feature flags are checked inside build.rs for sanity, so we
// can use them here assuming all targets are mutually exclusive.

#[cfg(feature = "precursor-perflib")]
#[rustfmt::skip]
mod precursor_perf;
#[cfg(feature = "precursor-perflib")]
#[rustfmt::skip]
pub use precursor_perf::*;

#[cfg(feature = "precursor-dvt")]
#[rustfmt::skip]
mod precursor_dvt;
#[cfg(feature = "precursor-dvt")]
#[rustfmt::skip]
pub use precursor_dvt::*;

#[cfg(feature = "renode")]
#[rustfmt::skip]
mod renode;
#[cfg(feature = "renode")]
#[rustfmt::skip]
pub use renode::*;

#[cfg(feature = "precursor-pvt")]
#[rustfmt::skip]
mod precursor_pvt;
#[cfg(feature = "precursor-pvt")]
#[rustfmt::skip]
pub use precursor_pvt::*;

#[cfg(feature = "cramium-soc")]
#[rustfmt::skip]
mod cramium_soc;
#[cfg(feature = "cramium-soc")]
#[rustfmt::skip]
pub use cramium_soc::*;

#[cfg(feature = "cramium-fpga")]
#[rustfmt::skip]
mod cramium_fpga;
#[cfg(feature = "cramium-fpga")]
#[rustfmt::skip]
pub use cramium_fpga::*;

#[cfg(feature = "atsama5d27")]
#[rustfmt::skip]
mod atsama5d27;
#[cfg(feature = "atsama5d27")]
#[rustfmt::skip]
pub use atsama5d27::*;

#[cfg(feature = "artybio")]
#[rustfmt::skip]
mod artybio;
#[cfg(feature = "artybio")]
#[rustfmt::skip]
pub use artybio::*;

// Hosted mode includes nothing, as it relies on the abstract host
// architecture for I/O; so this file is empty when the "hosted"
// configuration is selected and there are no corresponding "hosted"
// mode feature flags.
