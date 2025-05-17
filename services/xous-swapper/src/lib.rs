pub const PAGE_SIZE: usize = xous::arch::PAGE_SIZE;

/// userspace swapper -> kernel ABI
/// This ABI is copy-paste synchronized with what's in the kernel. It's left out of
/// xous-rs so that we can change it without having to push crates to crates.io.
/// Since there is only one place the ABI could be used, we're going to stick with
/// this primitive method of synchronization because it reduces the activation barrier
/// to fix bugs.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SwapAbi {
    Invalid = 0,
    ClearMemoryNow = 1,
    GetFreePages = 2,
    // RetrievePage = 3, // meant to be initiated within the kernel to itself
    // HardOom = 4, // meant to be initiated within the kernel to itself
    StealPage = 5,
    ReleaseMemory = 6,
    MarkDirty = 7,
    Sync = 8,
}
/// SYNC WITH `kernel/src/swap.rs`
impl SwapAbi {
    pub fn from(val: usize) -> SwapAbi {
        use SwapAbi::*;
        match val {
            1 => ClearMemoryNow,
            2 => GetFreePages,
            // 3 => RetrievePage,
            // 4 => HardOom,
            5 => StealPage,
            6 => ReleaseMemory,
            7 => MarkDirty,
            8 => Sync,
            _ => Invalid,
        }
    }
}

/// public userspace & swapper handler -> swapper userspace ABI
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(usize)]
pub enum Opcode {
    /// Userspace request to GC some physical pages
    GarbageCollect,
    /// Userspace request to sync FLASH memory to disk
    FlashSync,
    /// Test messages
    #[cfg(feature = "swap-userspace-testing")]
    Test0,
    None,
}

pub const SWAPPER_PUBLIC_NAME: &'static str = "_swapper server_";

pub struct Swapper {
    conn: xous::CID,
}
impl Swapper {
    pub fn new() -> Result<Self, xous::Error> {
        let xns = xous_api_names::XousNames::new().expect("couldn't connect to xous names");
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let conn =
            xns.request_connection_blocking(SWAPPER_PUBLIC_NAME).expect("Can't connect to TRNG server");
        Ok(Swapper { conn })
    }

    /// Attempts to free `page_count` pages of RAM.
    pub fn garbage_collect_pages(&self, page_count: usize) -> usize {
        match xous::send_message(
            self.conn,
            xous::Message::new_blocking_scalar(Opcode::GarbageCollect as usize, page_count, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(free_pages, _, _, _, _)) => free_pages,
            _e => {
                log::warn!("Garbage collect call failed with internal error: {:?}", _e);
                0
            }
        }
        // no result is given, but the call blocks until the GC call has completed in the swapper.
    }

    /// Sync needs to be called from within the context of the Swapper to allow for proper locking
    /// of the system while the sync happens.
    pub fn sync<T>(&self, region: Option<&[T]>) {
        if let Some(region) = region {
            let base = region.as_ptr() as usize;
            let len_bytes = region.len() * core::mem::size_of::<T>();
            xous::send_message(
                self.conn,
                xous::Message::new_blocking_scalar(
                    Opcode::FlashSync.to_usize().unwrap(),
                    1,
                    base,
                    len_bytes,
                    0,
                ),
            )
            .expect("Couldn't call sync on swapper");
        } else {
            xous::send_message(
                self.conn,
                xous::Message::new_blocking_scalar(Opcode::FlashSync.to_usize().unwrap(), 0, 0, 0, 0),
            )
            .expect("Couldn't call sync on swapper");
        }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};

use num_traits::ToPrimitive;
static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for Swapper {
    fn drop(&mut self) {
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}

/// This is safe to call from any context because the kernel call completes without activating
/// the swapper process - the pages are immediately marked dirty within the context of the calling process.
pub fn mark_dirty<T>(region: &[T]) {
    let base = region.as_ptr() as usize;
    let len_bytes = region.len() * core::mem::size_of::<T>();
    xous::rsyscall(xous::SysCall::SwapOp(SwapAbi::MarkDirty as usize, base, len_bytes, 0, 0, 0, 0))
        .expect("Couldn't mark region as dirty");
}
