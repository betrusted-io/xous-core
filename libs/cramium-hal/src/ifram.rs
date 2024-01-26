use xous::{send_message, MemoryRange, Message, Result};

pub enum IframBank {
    Bank0,
    Bank1,
}

/// `IframRange` is a range of memory that is suitable for use as a DMA target.
pub struct IframRange {
    pub(crate) phys_range: MemoryRange,
    pub(crate) virt_range: MemoryRange,
    /// The connection is optional, because in some special cases the range "outlives"
    /// the OS (e.g. serial ports handed to us from the loader), and thus also can't
    /// be "dropped".
    pub(crate) conn: Option<xous::CID>,
}

/// Constrain potential types for UDMA words to only what is representable and valid
/// for the UDMA subsystem.
pub trait UdmaWidths {}
impl UdmaWidths for i8 {}
impl UdmaWidths for u8 {}
impl UdmaWidths for i16 {}
impl UdmaWidths for u16 {}
impl UdmaWidths for i32 {}
impl UdmaWidths for u32 {}

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
        // This constant is in fact hard-coded because we are trying to break a circular
        // dependency on the cram-hal-service crate and make this library "neutral" so it
        // can be included in any context.
        let conn =
            xns.request_connection("_Cramium-SoC HAL_").expect("Couldn't connect to Cramium HAL server");
        let bank_code = match bank {
            Some(IframBank::Bank0) => 0,
            Some(IframBank::Bank1) => 1,
            _ => 2,
        };
        match send_message(conn, Message::new_blocking_scalar(0 /* MapIram */, length, bank_code, 0, 0)) {
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
                        phys_range: MemoryRange::new(maybe_phys_address, maybe_size).unwrap(),
                        virt_range: virtual_pages,
                        conn: Some(conn),
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

    /// Assemble a range from raw pointers. This function is highly unsafe and exists for the
    /// one special case of the logging crate, where we need to assemble a UART from raw, static
    /// parts. This allows us to have debug capabilities based on a UART that was initialized by
    /// the loader.
    pub unsafe fn from_raw_parts(phys_addr: usize, virt_addr: usize, size: usize) -> Self {
        IframRange {
            phys_range: MemoryRange::new(phys_addr, size).unwrap(),
            virt_range: MemoryRange::new(virt_addr, size).unwrap(),
            conn: None,
        }
    }

    /// Returns the IframRange as a slice, useful for passing to the UDMA API calls.
    /// This is `unsafe` because the slice is actually not accessible in virtual memory mode:
    /// any attempt to reference the returned slice will result in a panic. The returned
    /// slice is *only* useful as a range-checked base/bounds for UDMA API calls.
    pub unsafe fn as_phys_slice<T: UdmaWidths>(&self) -> &[T] { self.phys_range.as_slice::<T>() }

    pub fn as_slice<T: UdmaWidths>(&self) -> &[T] {
        // safe because `UdmaWidths` are always representable on our system
        unsafe { self.virt_range.as_slice::<T>() }
    }

    pub fn as_slice_mut<T: UdmaWidths>(&mut self) -> &mut [T] {
        // safe because `UdmaWidths` are always representable on our system
        unsafe { self.virt_range.as_slice_mut::<T>() }
    }
}

use core::sync::atomic::{AtomicU32, Ordering};
pub(crate) static REFCOUNT: AtomicU32 = AtomicU32::new(0);
impl Drop for IframRange {
    fn drop(&mut self) {
        // this is all terrible and broken, see https://github.com/betrusted-io/xous-core/issues/482
        // there's also now a race condition on the connection "take" on Drop, but...let's deal
        // with that after 482 is dealt with.
        if let Some(conn) = self.conn.take() {
            match send_message(
                conn,
                Message::new_blocking_scalar(
                    1, /* UnmapIfram */
                    self.phys_range.len(),
                    self.phys_range.as_ptr() as usize,
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
            // de-allocate myself. It's unsafe because we are responsible to make sure nobody else is using
            // the connection.
            if REFCOUNT.fetch_sub(1, Ordering::Relaxed) == 1 {
                unsafe {
                    xous::disconnect(conn).unwrap();
                }
            } else {
                // replace the connection, since it's still in use.
                self.conn = Some(conn);
            }
        }
    }
}
