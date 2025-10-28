use utralib::{CSR, utra};

#[cfg(not(feature = "std"))]
pub struct Reram {
    csr: CSR<u32>,
    array: &'static mut [u32],
}

/// Matches the alignment requirement of the RRC write buffer
const ALIGNMENT: usize = 32;
#[cfg(feature = "std")]
pub struct Reram {
    csr: CSR<u32>,
    irq_csr: CSR<u32>,
    offset: usize,
    buf: [u32; ALIGNMENT / size_of::<u32>()],

    /// Replace this with a heap-allocated object that tracks a set of
    /// memory ranges that we are allowed to write to. The Reram object has to be owned/managed
    /// by the highest security process in Xous. This is because the process that can initiate
    /// writes also has to have page-level access to the memory that's write-able; therefore,
    /// it must be managed by the most privileged process in the system.
    ///
    /// Upon write, the allowed memory ranges are checked, and an error is thrown if the area
    /// is not mapped. In particular, program areas are not writeable in Xous run-time mode
    /// because they are mapped into the process' memory space (to allow for XIP code access).
    ///
    /// Another quirk for `std` memory access is that the core write needs to happen inside an
    /// interrupt context: if the write is interrupted and another process touches memory
    /// between the staging and the commit of data, we can have an inconsistency in write state
    /// and corruption will result.
    range_map: RangeMap<xous::MemoryRange>,
}

#[repr(align(4))]
struct AlignedBuffer([u8; ALIGNMENT]);
impl AlignedBuffer {
    pub fn as_slice_u32(&self) -> &[u32] {
        // safety: this is safe because the #repr(align) ensures that our alignment is correct,
        // and the length of the internal data structure is set correctly by design. Furthermore,
        // all values in both the source and destination transmutation are representable and valid.
        // The structure has no concurrent uses and no need for a Drop.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr() as *const u32, self.0.len() / 4) }
    }
}

/// This is the code that enables the security modes. Must be written into
/// RRCR on every update - kind of dangerous design, because it is too easy
/// to overlook setting this in a compound register.
pub const SECURITY_MODE: u32 = 0b1111_1100_0000_0000;
const RRC_LOAD_BUFFER: u32 = 0x5200;
const RRC_WRITE_BUFFER: u32 = 0x9528;
const RRC_CR_NORMAL: u32 = 0;
#[allow(dead_code)]
const RRC_CR_POWERDOWN: u32 = 1;
#[allow(dead_code)]
const RRC_CR_WRITE_DATA: u32 = 0;
const RRC_CR_WRITE_CMD: u32 = 2;

#[cfg(feature = "std")]
fn rram_handler(_irq_no: usize, arg: *mut usize) {
    let rram = unsafe { &mut *(arg as *mut Reram) };
    // clear the interrupt pending
    rram.irq_csr.wo(utra::irqarray11::EV_PENDING, rram.irq_csr.r(utra::irqarray11::EV_PENDING));

    assert!(rram.offset % 0x20 == 0, "unaligned destination address!");
    assert!(rram.buf.len() % 8 == 0, "unaligned source data!");

    // local copies of the RRAM data so we can do a mut reference in the loop below
    let offset = rram.offset;
    let mut buf = [0u32; ALIGNMENT / size_of::<u32>()];
    buf.copy_from_slice(&rram.buf);
    let base_ptr = rram.array_mut(offset).unwrap().as_mut_ptr();
    for (inner, &datum) in buf.iter().enumerate() {
        unsafe {
            base_ptr.add(inner).write_volatile(datum);
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
    rram.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_WRITE_CMD | SECURITY_MODE);
    unsafe {
        base_ptr.write_volatile(RRC_LOAD_BUFFER);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        base_ptr.write_volatile(RRC_WRITE_BUFFER);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        rram.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_NORMAL | SECURITY_MODE);
    }
    crate::cache_flush();
}

impl<'a> Reram {
    #[cfg(not(feature = "std"))]
    pub fn new() -> Self {
        let mut csr = CSR::new(utra::rrc::HW_RRC_BASE as *mut u32);
        // this enables access control protections. In metal-mask stepping A1, this will
        // be hard-wired as enabled without an option to turn it off.
        csr.wo(utra::rrc::SFR_RRCCR, SECURITY_MODE);

        Reram {
            csr,
            array: unsafe {
                core::slice::from_raw_parts_mut(
                    utralib::HW_RERAM_MEM as *mut u32,
                    utralib::HW_RERAM_MEM_LEN / core::mem::size_of::<u32>(),
                )
            },
        }
    }

    #[cfg(feature = "std")]
    /// This returns an object that has a handle to manage the RRAM, but no memory
    /// ranges mapped that are valid for writing. Memory ranges need to be added
    /// with `add_range()` before any writes can occur.
    pub fn new() -> Self {
        let rram_page = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::rrc::HW_RRC_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't claim rram page");

        let mut csr = CSR::new(rram_page.as_mut_ptr() as *mut u32);
        // this enables access control protections. In metal-mask stepping A1, this will
        // be hard-wired as enabled without an option to turn it off.
        csr.wfo(utra::rrc::SFR_RRCCR_SFR_RRCCR, SECURITY_MODE);

        let irq_page = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::irqarray11::HW_IRQARRAY11_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't claim irq page");
        let mut irq_csr = CSR::new(irq_page.as_mut_ptr() as *mut u32);

        // pin the structure on the heap so that it doesn't move around on us.
        let mut rram = Reram {
            csr,
            irq_csr,
            offset: 0,
            buf: [0u32; ALIGNMENT / size_of::<u32>()],
            range_map: RangeMap::new(),
        };

        xous::claim_interrupt(
            utra::irqarray11::IRQARRAY11_IRQ,
            rram_handler,
            &mut rram as *mut Reram as *mut usize,
        )
        .expect("couldn't claim RRAM handler interrupt");
        irq_csr.wo(utra::irqarray11::EV_PENDING, 0xFFFF_FFFF);
        irq_csr.wo(utra::irqarray11::EV_EDGE_TRIGGERED, 1 << 15);
        irq_csr.wo(utra::irqarray11::EV_POLARITY, 1 << 15);
        // enable just bit 15 for use as the software interrupt field
        irq_csr.wfo(utra::irqarray11::EV_ENABLE_NC_B11S15, 1);

        rram
    }

    #[cfg(feature = "std")]
    /// `base` specifies the actual offset from the beginning of the RRAM array
    /// to where this range maps to. This is necessary because `range` is virtually
    /// mapped and we cannot infer the actual offset into the RRAM array from
    /// that artifact alone.
    pub fn add_range(&'a mut self, base_offset: usize, range: xous::MemoryRange) -> &'a Self {
        let top = base_offset + range.len();
        self.range_map.insert(base_offset..top, range);
        self
    }

    #[cfg(not(feature = "std"))]
    pub fn read_slice(&self) -> &[u32] { self.array }

    /// Safety: the write destination address must be aligned to a 256-bit boundary, and the data
    /// must be exactly 256 bits long.
    ///
    /// It's also not safe to call in any context where there can be concurrency.
    #[cfg(not(feature = "std"))]
    pub unsafe fn write_u32_aligned(&mut self, addr: usize, data: &[u32]) {
        assert!(addr % 0x20 == 0, "unaligned destination address!");
        assert!(data.len() % 8 == 0, "unaligned source data!");
        // crate::print!("@ {:x} > ", addr);
        for (outer, d) in data.chunks_exact(8).enumerate() {
            // write the data to the buffer
            for (inner, &datum) in d.iter().enumerate() {
                // crate::print!(" {:x}", datum);
                self.array
                    .as_mut_ptr()
                    .add(addr / core::mem::size_of::<u32>() + outer * 8 + inner)
                    .write_volatile(datum);
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            }
            // crate::println!("");

            self.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_WRITE_CMD | SECURITY_MODE);
            self.array
                .as_mut_ptr()
                .add(addr / core::mem::size_of::<u32>() + outer * 8)
                .write_volatile(RRC_LOAD_BUFFER);
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            self.array
                .as_mut_ptr()
                .add(addr / core::mem::size_of::<u32>() + outer * 8)
                .write_volatile(RRC_WRITE_BUFFER);
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            self.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_NORMAL | SECURITY_MODE);
        }
        crate::cache_flush();
    }

    #[cfg(feature = "std")]
    /// Safety: the write destination address must be aligned to a 256-bit boundary, and the data
    /// must be exactly 256 bits long.
    unsafe fn write_u32_aligned(&mut self, addr: usize, data: &[u32]) {
        // copy artifacts for the interrupt handler
        self.offset = addr;
        self.buf.copy_from_slice(&data);
        // trigger the interrupt
        self.irq_csr.wo(utra::irqarray11::EV_SOFT, 1 << 15);
    }

    #[cfg(feature = "std")]
    /// This returns u8-slice for filling in missing data
    pub fn array(&self, offset: usize) -> Result<&[u8], xous::Error> {
        if let Some((start_offset, range)) = self.range_map.get(offset) {
            // safety: all values of u8 are valid; data is strictly read-only and static
            // so there are no concurrency or Drop issues
            let range_slice: &[u8] = unsafe { range.as_slice() };
            /*
            crate::println!(
                "array found {:x} mapping for offset {:x}",
                start_offset,
                range_slice.as_ptr() as usize
            ); */
            // offset is based from bottom of RRAM. Correct for this.
            Ok(&range_slice[offset - start_offset..offset - start_offset + ALIGNMENT])
        } else {
            Err(xous::Error::AccessDenied)
        }
    }

    #[cfg(feature = "std")]
    /// This returns u32-slice for doing writes
    fn array_mut(&mut self, offset: usize) -> Result<&mut [u32], xous::Error> {
        if let Some((start_offset, range)) = self.range_map.get_mut(offset) {
            // safety: all values of u8 are valid; data is strictly read-only and static
            // so there are no concurrency or Drop issues
            let range_slice: &mut [u32] = unsafe { range.as_slice_mut() };
            // offset is based from bottom of RRAM. Correct for this.
            let offset_words = (offset - start_offset) / size_of::<u32>();
            /*
            crate::println!(
                "mut array found {:x} mapping for offset {:x} (as word {:x}",
                start_offset,
                range_slice.as_ptr() as usize,
                offset_words
            ); */
            Ok(&mut range_slice[offset_words..offset_words + ALIGNMENT / size_of::<u32>()])
        } else {
            Err(xous::Error::AccessDenied)
        }
    }

    /// This is a general unaligned write primitive for the RRAM that can handle any length
    /// slice and alignment of data.
    ///
    /// ASSUME: offset has been bounds checked by a wrapper function.
    fn write_slice_inner(&mut self, offset: usize, data: &[u8]) -> Result<usize, xous::Error> {
        let mut buffer = AlignedBuffer([0u8; ALIGNMENT]);

        // ragged start
        let start_len = (ALIGNMENT - (offset % ALIGNMENT)) % ALIGNMENT;
        if start_len != 0 {
            let start_offset = offset & !(ALIGNMENT - 1);
            #[cfg(not(feature = "std"))]
            let dest_slice = unsafe {
                core::slice::from_raw_parts(
                    (start_offset + utralib::HW_RERAM_MEM) as *const u8,
                    buffer.0.len(),
                )
            };
            #[cfg(feature = "std")]
            let dest_slice = self.array(offset)?;
            // crate::println!("original data @ {:x}: {:x?}", start_offset, &dest_slice);
            // populate from old data first
            buffer.0.copy_from_slice(&dest_slice);
            for (dst, &src) in
                buffer.0[offset % ALIGNMENT..].iter_mut().zip(data[..start_len.min(data.len())].iter())
            {
                *dst = src;
            }
            // crate::println!("ragged start {:x?}; data {:x?}", buffer.0, data);
            // safe because alignment and buffer sizes are guaranteed
            unsafe {
                self.write_u32_aligned(start_offset, buffer.as_slice_u32());
            }
        }

        // aligned middle & end
        let mut cur_offset = offset + start_len;
        if data.len().saturating_sub(start_len) > 0 {
            for chunk in data[start_len..].chunks(buffer.0.len()) {
                // full chunk
                if chunk.len() == buffer.0.len() {
                    buffer.0.copy_from_slice(&chunk);
                    // safe because alignment and buffer sizes are guaranteed
                    // crate::println!("aligned mid {:x?}; data {:x?}", buffer.0, data);
                    unsafe {
                        self.write_u32_aligned(cur_offset, &buffer.as_slice_u32());
                    }
                } else {
                    #[cfg(not(feature = "std"))]
                    let dest_slice = unsafe {
                        core::slice::from_raw_parts(
                            (cur_offset + utralib::HW_RERAM_MEM) as *const u8,
                            buffer.0.len(),
                        )
                    };
                    #[cfg(feature = "std")]
                    let dest_slice = self.array(offset)?;
                    // read in the destination full contents
                    // crate::println!("original data @ {:x}: {:x?}", cur_offset, &dest_slice);
                    buffer.0.copy_from_slice(&dest_slice);
                    // now overwrite the "ragged end"
                    buffer.0[..chunk.len()].copy_from_slice(&chunk);
                    // safe because alignment and buffer sizes are guaranteed
                    // crate::println!("ragged end {:x?}; data {:x?}", buffer.0, data);
                    unsafe {
                        self.write_u32_aligned(cur_offset, &buffer.as_slice_u32());
                    }
                }
                cur_offset += chunk.len();
            }
        }

        // QUESTION: do we want to add a mandatory readback-verify here?
        Ok(data.len())
    }

    /// Bounds-check regular writes to make sure they are in the "standard" memory array region
    /// Boot0 region is also disallowed for writing because it should be hardware write-protected.
    /// For testing purposes, that check may be commented out just to make sure the write protection
    /// is there.
    pub fn write_slice(&mut self, offset: usize, data: &[u8]) -> Result<usize, xous::Error> {
        // This needs to be disabled for CI tests that check if boot0 is actually hardware
        // write-protected (because otherwise software would just fail at this check).
        #[cfg(not(feature = "redteam"))]
        if offset < bao1x_api::offsets::BOOT1_START - utralib::HW_RERAM_MEM
            || offset >= bao1x_api::RRAM_STORAGE_LEN
        {
            return Err(xous::Error::AccessDenied);
        }
        self.write_slice_inner(offset, data)
    }

    /// This has a separate API not to enforce security but to avoid fat-fingering data
    /// into secure sectors. Hence this is just a simple bounds check on the requested offset.
    /// There are nominally other hardware mechanisms at play to disallow writes from ineligible
    /// processes, but they only come into effect after the OS is booted.
    pub fn protected_write_slice(&mut self, offset: usize, data: &[u8]) -> Result<usize, xous::Error> {
        if offset < bao1x_api::RRAM_STORAGE_LEN || offset >= utralib::HW_RERAM_MEM_LEN {
            return Err(xous::Error::AccessDenied);
        }
        self.write_slice_inner(offset, data)
    }
}

#[cfg(feature = "std")]
use std::ops::Range;

#[derive(Debug)]
#[cfg(feature = "std")]
struct RangeMap<T> {
    ranges: Vec<(Range<usize>, T)>,
}

#[cfg(feature = "std")]
impl<T> RangeMap<T> {
    pub fn new() -> Self { Self { ranges: Vec::new() } }

    pub fn insert(&mut self, range: Range<usize>, value: T) { self.ranges.push((range, value)); }

    /// Binary search. Ranges must be non-overlapping *and sorted* in ascending order. I think
    /// we don't have to get this fancy as the number of ranges is probably small, and we can't
    /// guarantee the sorted order of ranges without some sort of "finalize" operation.
    #[allow(dead_code)]
    pub fn get_binary(&self, x: usize) -> Option<(usize, &T)> {
        let idx = self.ranges.binary_search_by_key(&x, |(r, _)| r.start).unwrap_or_else(|i| i);
        let i = if idx == 0 { 0 } else { idx - 1 };
        self.ranges.get(i).and_then(|(r, v)| (r.start <= x && x < r.end).then_some((r.start, v)))
    }

    // linear search. Ranges must be non-overlappping but can be presented in any order.
    pub fn get(&self, x: usize) -> Option<(usize, &T)> {
        self.ranges.iter().find(|(r, _)| r.start <= x && x < r.end).map(|(r, v)| (r.start, v))
    }

    // linear get_mut
    pub fn get_mut(&mut self, x: usize) -> Option<(usize, &mut T)> {
        self.ranges.iter_mut().find(|(r, _)| r.start <= x && x < r.end).map(|(r, v)| (r.start, v))
    }
}
