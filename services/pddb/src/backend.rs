mod basis;
pub use basis::*;
mod dictionary;
pub use dictionary::*;
mod key;
pub use key::*;
mod pagetable;
pub use pagetable::*;
mod fastspace;
pub use fastspace::*;
mod types;
pub use types::*;


// local to the backend
mod murmur3;
pub(crate) use murmur3::*;
mod trngpool;
pub(crate) use trngpool::*;

mod hw;
pub(crate) use hw::*;

// hosted mode emulation structures
//#[cfg(not(any(target_os = "none", target_os = "xous")))]
mod hosted;
//#[cfg(not(any(target_os = "none", target_os = "xous")))]
pub(crate) use hosted::*;
