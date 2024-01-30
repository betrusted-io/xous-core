#[cfg(any(feature = "precursor", feature = "renode"))]
pub mod dns;
#[cfg(any(feature = "precursor", feature = "renode"))]
pub use dns::*;
#[cfg(not(target_os = "xous"))]
pub mod dns_hosted;
#[cfg(not(target_os = "xous"))]
pub use dns_hosted::*;

pub mod ping;
pub use ping::*;
