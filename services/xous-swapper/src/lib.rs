#[cfg(feature = "cramium-soc")]
use cramium_hal::board::SPINOR_ERASE_SIZE;
#[cfg(any(feature = "precursor", feature = "renode"))]
use precursor_hal::board::SPINOR_ERASE_SIZE;

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
    WritePage = 7,
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
            7 => WritePage,
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
    /// This call will only process full-page writes to an offset in SPI FLASH.
    /// The full-page criteria is acceptable because (a) we need the full page anyways
    /// due to the sector erase constraint and (b) the caller has to have the full page
    /// anyways because the page was read in and mapped into the caller's space at some
    /// point in time.
    WritePage,
    /// Test messages
    #[cfg(feature = "swap-userspace-testing")]
    Test0,
    None,
}

/// An aligned structure for sending FLASH data between structures. This only works
/// if SPINOR_ERASE_SIZE == 4096.
#[repr(C, align(4096))]
pub struct FlashPage<const N: usize = { SPINOR_ERASE_SIZE as usize }> {
    pub data: [u8; N],
}
impl FlashPage {
    pub fn new() -> Self { Self { data: [0u8; SPINOR_ERASE_SIZE as usize] } }
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

    /// `offset` is transmitted with the address bank mask for MMAP_VIRT attached to avoid
    /// the first sector falling foul of the NonZero requirement.
    pub fn write_page(&self, offset: usize, page: FlashPage) -> Result<xous::Result, xous::Error> {
        if (offset & (cramium_hal::board::SPINOR_ERASE_SIZE as usize - 1)) != 0 {
            return Err(xous::Error::BadAddress);
        }
        let msg = MemoryMessage {
            id: Opcode::WritePage.to_usize().unwrap(),
            // safety: page.data is guaranteed to be aligned due to the repr(C) alignment directive
            // also, all values are representable as a u8.
            buf: unsafe {
                MemoryRange::new(page.data.as_ptr() as usize, SPINOR_ERASE_SIZE as usize).unwrap()
            },
            offset: NonZero::new(offset),
            valid: None, // unused, the whole page is valid by definition
        };
        // we do a Borrow instead of a Move not because we care about the return value,
        // but because we care that the call finished before proceeding.
        xous::send_message(self.conn, xous::Message::Borrow(msg))
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
use std::num::NonZero;

use num_traits::ToPrimitive;
use xous::{MemoryMessage, MemoryRange};
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
