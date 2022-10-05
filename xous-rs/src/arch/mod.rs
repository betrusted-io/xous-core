#[cfg(any(feature="precursor", feature="renode"))]
pub mod riscv;
#[cfg(any(feature="precursor", feature="renode"))]
pub use riscv::*;

#[cfg(any(all(feature="hosted", not(feature = "processes-as-threads")),
    not(any(feature="precursor", feature="renode"))
))]
pub mod hosted;
#[cfg(any(all(feature="hosted", not(feature = "processes-as-threads")),
    not(any(feature="precursor", feature="renode"))
))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
