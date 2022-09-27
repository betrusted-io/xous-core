// Feature flags are checked inside build.rs for sanity, so we
// can use them here assuming all targets are mutually exclusive.

#[cfg(feature="precursor-c809403")]
mod precursor_c809403;
#[cfg(feature="precursor-c809403")]
pub use precursor_c809403::*;


#[cfg(feature="precursor-c809403-perflib")]
mod precursor_perf_c809403;
#[cfg(feature="precursor-c809403-perflib")]
pub use precursor_perf_c809403::*;

#[cfg(feature="renode")]
mod renode;
#[cfg(feature="renode")]
pub use renode::*;

// Hosted mode includes nothing, as it relies on the abstract host
// architecture for I/O; so this file is empty when the "hosted"
// configuration is selected and there are no corresponding "hosted"
// mode feature flags.