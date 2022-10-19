// Feature flags are checked inside build.rs for sanity, so we
// can use them here assuming all targets are mutually exclusive.

#[cfg(feature="precursor-c809403")]
mod precursor_c809403;
#[cfg(feature="precursor-c809403")]
pub use precursor_c809403::*;

#[cfg(feature="precursor-6156e49")]
mod precursor_6156e49;
#[cfg(feature="precursor-6156e49")]
pub use precursor_6156e49::*;

#[cfg(feature="precursor-c809403-perflib")]
mod precursor_perf_c809403;
#[cfg(feature="precursor-c809403-perflib")]
pub use precursor_perf_c809403::*;

#[cfg(feature="precursor-2753c12-dvt")]
mod precursor_dvt_2753c12;
#[cfg(feature="precursor-2753c12-dvt")]
pub use precursor_dvt_2753c12::*;

#[cfg(feature="renode")]
mod renode;
#[cfg(feature="renode")]
pub use renode::*;

// Hosted mode includes nothing, as it relies on the abstract host
// architecture for I/O; so this file is empty when the "hosted"
// configuration is selected and there are no corresponding "hosted"
// mode feature flags.