#[cfg(all(target_os = "xous", target_arch = "arm"))]
mod arm;
#[cfg(all(target_os = "xous", target_arch = "arm"))]
pub use arm::*;

#[cfg(all(target_os = "xous", target_arch = "riscv32"))]
pub mod riscv;
#[cfg(all(target_os = "xous", target_arch = "riscv32"))]
pub use riscv::*;

#[cfg(all(
    not(feature="processes-as-threads"),
    not(target_os = "xous"),
    not(target_arch = "arm"),
))]
pub mod hosted;
#[cfg(all(
    not(feature="processes-as-threads"),
    not(target_os = "xous"),
    not(target_arch = "arm"),
))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
