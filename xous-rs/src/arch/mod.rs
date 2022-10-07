#[cfg(any(feature="precursor", feature="renode"))]
pub mod riscv;
#[cfg(any(feature="precursor", feature="renode"))]
pub use riscv::*;

#[cfg(all(
    not(feature="processes-as-threads"),
    any(feature="hosted",
        not(any(feature="precursor", feature="renode"))
    )
))]
pub mod hosted;
#[cfg(all(
    not(feature="processes-as-threads"),
    any(feature="hosted",
        not(any(feature="precursor", feature="renode"))
    )
))]
pub use hosted::*;

#[cfg(feature = "processes-as-threads")]
pub mod test;
#[cfg(feature = "processes-as-threads")]
pub use test::*;
