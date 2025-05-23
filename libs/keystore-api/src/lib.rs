// legacy API
#[cfg(feature = "gen1")]
pub mod rootkeys_api;
#[cfg(feature = "gen1")]
pub use rootkeys_api::*;
// gen2 API
#[cfg(feature = "gen2")]
pub mod gen2_api;
#[cfg(feature = "gen2")]
pub use gen2_api::*;

pub mod common;
pub use common::*;
pub mod rkyv_enum;
