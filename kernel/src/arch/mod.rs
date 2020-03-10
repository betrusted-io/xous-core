
#[cfg(target_arch = "riscv32")]
mod riscv;
#[cfg(target_arch = "riscv32")]
pub use riscv::*;

#[cfg(target_arch = "riscv64")]
mod riscv;
#[cfg(target_arch = "riscv64")]
pub use riscv::*;

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;
