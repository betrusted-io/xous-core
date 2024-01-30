// As of Rust 1.64.0:
//
// Rkyv-derived enums throw warnings that rkyv::Archive derived enums are never used
// and I can't figure out how to make them go away. Since they spam the build log,
// rkyv-derived enums are now isolated to their own file with a file-wide `dead_code`
// allow on top.
//
// This might be a temporary compiler regression, or it could just
// be yet another indicator that it's time to upgrade rkyv. However, we are waiting
// until rkyv hits 0.8 (the "shouldn't ever change again but still not sure enough
// for 1.0") release until we rework the entire system to chase the latest rkyv.
// As of now, the current version is 0.7.x and there isn't a timeline yet for 0.8.
#![allow(dead_code)]

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum FlashOp {
    /// erase a region defined by (address, len)
    Erase(u32, u32),
    /// Send up to 1kiB of data at a time. This reduces messaging overhead and makes
    /// programming more efficient, while taking full advantage of the 1280-deep receive FIFO on the EC.
    /// Address + up to 4 pages. page 0 is at address, page 1 is at address + 256, etc.
    /// Pages stored as None are skipped, yet the address pointer is still incremented.
    Program(u32, [Option<[u8; 256]>; 4]),
    /// Read a data at the `u32` address specified.
    Verify(u32, [u8; 256]),
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum FlashResult {
    Pass,
    Fail,
}
