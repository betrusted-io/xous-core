use num_traits::*;
use xous::{send_message, MemoryAddress, MemoryRange, MemorySize, Message, Result};

use crate::{Opcode, SERVER_NAME_CRAM_HAL};

pub enum IframBank {
    Bank0,
    Bank1,
}

/// `IframRange` is a range of memory that is suitable for use as a DMA target.
pub struct IframRange {
    pub(crate) phys_addr: MemoryAddress,
    pub(crate) memory: MemoryRange,
    pub(crate) size: MemorySize,
    pub(crate) conn: xous::CID,
}

impl IframRange {
    /// Request `length` bytes of memory in an optional Bank
    ///
    /// Safety: the caller must ensure that the `IframRange` object lives for the
    /// entire lifetime of the *hardware* operation.
    ///
    /// Example of how things can go wrong: `IframRange` is used to allocate
    /// a buffer for data that takes a long time to send via a slow UART.
    /// The DMA is initiated, and the function exits, dropping `IframRange`.
    /// At this point, the range could be re-allocated for another purpose.
    ///
    /// An `IframRange` would naturally live as long as a DMA request
    /// if a sender synchronizes on the completion of the DMA request. Thus,
    /// the `unsafe` bit happens in "fire-and-forget" contexts, and the
    /// caller has to explicitly manage the lifetime of this object to
    /// match the maximum duration of the call.
    ///
    /// A simple way to do this is to simply bind the structure to a long-lived
    /// scope, but the trade-off is that IFRAM is very limited in capacity
    /// and hanging on to chunks much longer than necessary can lead to memory
    /// exhaustion.
    pub unsafe fn request(length: usize, bank: Option<IframBank>) -> Option<Self> {
        REFCOUNT.fetch_add(1, Ordering::Relaxed);
        let xns = xous_api_names::XousNames::new().unwrap();
        let conn =
            xns.request_connection(SERVER_NAME_CRAM_HAL).expect("Couldn't connect to Cramium HAL server");
        let bank_code = match bank {
            Some(IframBank::Bank0) => 0,
            Some(IframBank::Bank1) => 1,
            _ => 2,
        };
        match send_message(
            conn,
            Message::new_blocking_scalar(Opcode::MapIfram.to_usize().unwrap(), length, bank_code, 0, 0),
        ) {
            Ok(Result::Scalar5(maybe_size, maybe_phys_address, _, _, _)) => {
                if maybe_size != 0 && maybe_phys_address != 0 {
                    let mut page_aligned_size = maybe_size / 4096;
                    if maybe_size % 4096 != 0 {
                        page_aligned_size += 1;
                    }
                    let virtual_pages = xous::map_memory(
                        core::num::NonZeroUsize::new(maybe_phys_address),
                        None,
                        page_aligned_size,
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    )
                    .unwrap();
                    Some(IframRange {
                        phys_addr: MemoryAddress::new(maybe_phys_address).unwrap(),
                        memory: virtual_pages,
                        size: MemorySize::new(maybe_size).unwrap(),
                        conn,
                    })
                } else {
                    if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
                        // "almost" safe because we checked our reference count before disconnecting
                        // there is a race condition possibly if someone allocates a connection between
                        // the check above, and the disconnect below.
                        unsafe { xous::disconnect(conn).unwrap() }
                    };
                    None
                }
            }
            _ => {
                if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
                    // "almost" safe because we checked our reference count before disconnecting
                    // there is a race condition possibly if someone allocates a connection between
                    // the check above, and the disconnect below.
                    unsafe { xous::disconnect(conn).unwrap() }
                };
                None
            }
        }
    }

    /// Returns the IframRange as a slice, useful for passing to the UDMA API calls.
    /// This is `unsafe` because the slice is actually not accessible in virtual memory mode:
    /// any attempt to reference the returned slice will result in a panic. The returned
    /// slice is *only* useful as a range-checked base/bounds for UDMA API calls.
    pub unsafe fn as_phys_slice(&self) -> &[u8] {
        core::slice::from_raw_parts(self.phys_addr.get() as *const u8, self.size.get())
    }

    pub fn as_slice(&self) -> &[u8] {
        // safe because `u8` is always representable on our system
        unsafe { self.memory.as_slice() }
    }

    pub fn as_slice_u32(&self) -> &[u32] {
        // safe because `u32` is always representable on our system
        unsafe { self.memory.as_slice() }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        // safe because `u8` is always representable on our system
        unsafe { self.memory.as_slice_mut() }
    }

    pub fn as_slice_u32_mut(&mut self) -> &mut [u32] {
        // safe because `u32` is always representable on our system
        unsafe { self.memory.as_slice_mut() }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
pub(crate) static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for IframRange {
    fn drop(&mut self) {
        match send_message(
            self.conn,
            Message::new_blocking_scalar(
                Opcode::UnmapIfram.to_usize().unwrap(),
                self.size.get(),
                self.phys_addr.get(),
                0,
                0,
            ),
        ) {
            Ok(_) => (),
            Err(e) => {
                // This probably never happens, but also probably doesn't need to be a hard-panic
                // if it does happen because it simply degrades performance; it does not impact
                // correctness.
                log::error!("Couldn't de-allocate IframRange: {:?}, IFRAM memory is leaking!", e)
            }
        }
        // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using the
        // connection.
        if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
            unsafe {
                xous::disconnect(self.conn).unwrap();
            }
        }
    }
}
