// Feature flags are checked inside build.rs for sanity, so we
// can use them here assuming all targets are mutually exclusive.

#[cfg(feature="precursor-c809403")]
mod precursor_c809403;
#[cfg(feature="precursor-c809403")]
pub use precursor_c809403::*;

#[cfg(feature="precursor-a0912d6")]
mod precursor_a0912d6;
#[cfg(feature="precursor-a0912d6")]
pub use precursor_a0912d6::*;

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

#[cfg(feature="precursor-b414f2b")]
mod precursor_b414f2b;
#[cfg(feature="precursor-b414f2b")]
pub use precursor_b414f2b::*;

// Hosted mode includes nothing, as it relies on the abstract host
// architecture for I/O; so this file is empty when the "hosted"
// configuration is selected and there are no corresponding "hosted"
// mode feature flags.