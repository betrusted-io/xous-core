// legacy API
#[cfg(feature = "gen1")]
pub mod rootkeys_api;
#[cfg(feature = "gen1")]
pub use rootkeys_api::TOTAL_CHECKSUMS;
// gen2 API
#[cfg(feature = "gen2")]
pub mod keystore_api;
#[cfg(feature = "gen2")]
pub use keystore_api::TOTAL_CHECKSUMS;

pub mod common;
pub use common::*;
pub mod rkyv_enum;
