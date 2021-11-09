mod basis;
pub use basis::*;
mod pagetable;
pub use pagetable::*;
mod fastspace;
pub use fastspace::*;
mod types;
pub use types::*;


// local to the backend
mod murmur3;
pub(crate) use murmur3::*;

#[cfg(any(target_os = "none", target_os = "xous"))]
mod hw;
pub(crate) use hw::*;

// TODO: the alternative back-end PddbOs structures for hosted mode.