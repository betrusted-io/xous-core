// note: cramium-soc is a bit of a misplaced feature as it is a chip not a platform,
// but it's included here to make vscode code analyzer a bit happier
#[cfg(any(feature = "board-baosec", feature = "hosted-baosec"))]
mod baosec;
#[cfg(any(feature = "board-baosec", feature = "hosted-baosec"))]
pub use baosec::*;

#[cfg(any(feature = "hosted", feature = "renode", feature = "precursor"))]
mod precursor;
#[cfg(any(feature = "hosted", feature = "renode", feature = "precursor"))]
pub use precursor::*;

// Dummy configuration to allow cargo doc to run - this has no board specified
#[cfg(any(
    all(
        not(any(feature = "board-baosec", feature = "hosted-baosec")),
        not(any(feature = "hosted", feature = "renode", feature = "precursor"))
    ),
    doc
))]
mod doc;

#[cfg(any(
    all(
        not(any(feature = "board-baosec", feature = "hosted-baosec")),
        not(any(feature = "hosted", feature = "renode", feature = "precursor"))
    ),
    doc
))]
pub use doc::*;
