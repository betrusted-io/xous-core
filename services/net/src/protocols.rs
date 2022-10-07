#[cfg(any(feature="precursor", feature="renode"))]
pub mod dns;
#[cfg(any(feature="precursor", feature="renode"))]
pub use dns::*;
#[cfg(any(feature="hosted"))]
pub mod dns_hosted;
#[cfg(any(feature="hosted"))]
pub use dns_hosted::*;

pub mod ping;
pub use ping::*;
