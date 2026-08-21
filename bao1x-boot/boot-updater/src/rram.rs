// vendor in an rram writer that has no bounds checks. We don't feature-flag it or otherwise try
// to expose it in the primary RRAM API because it is too unsafe to just leave laying around as
// an option that someone can bumble into with a typo.

use utralib::{CSR, utra};

pub struct Reram {
    csr: CSR<u32>,
    array: &'static mut [u32],
}

/// Matches the alignment requirement of the RRC write buffer
const ALIGNMENT: usize = 32;

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

/// This disables the security. Only works on A0 silicon. Required to bypass security, which then
/// allows for updates of boot0
#[cfg(feature = "boot0")]
pub const SECURITY_MODE: u32 = 0x0;
/// This is the code that enables the security modes. Must be written into
/// RRCR on every update - kind of dangerous design, because it is too easy
/// to overlook setting this in a compound register.
#[cfg(not(feature = "boot0"))]
pub const SECURITY_MODE: u32 = 0b1111_1100_0000_0000;

const RRC_LOAD_BUFFER: u32 = 0x5200;
const RRC_WRITE_BUFFER: u32 = 0x9528;
const RRC_CR_NORMAL: u32 = 0;
#[allow(dead_code)]
const RRC_CR_POWERDOWN: u32 = 1;
#[allow(dead_code)]
const RRC_CR_WRITE_DATA: u32 = 0;
const RRC_CR_WRITE_CMD: u32 = 2;

// number of attempts allowed before giving up on writes
const ATTEMPTS: usize = 2;

impl<'a> Reram {
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

    /// Safety: the write destination address must be aligned to a 256-bit boundary, and the data
    /// must be exactly 256 bits long.
    ///
    /// It's also not safe to call in any context where there can be concurrency.
    pub unsafe fn write_u32_aligned(&mut self, addr: usize, data: &[u32]) {
        assert!(addr % 0x20 == 0, "unaligned destination address!");
        assert!(data.len() % 8 == 0, "unaligned source data!");
        for (outer, d) in data.chunks_exact(8).enumerate() {
            bao1x_api::bollard!(bao1x_hal::sigcheck::die_no_std, 4);
            // write the data to the buffer
            for (inner, &datum) in d.iter().enumerate() {
                bao1x_api::bollard!(bao1x_hal::sigcheck::die_no_std, 4);
                unsafe {
                    self.array
                        .as_mut_ptr()
                        .add(addr / core::mem::size_of::<u32>() + outer * 8 + inner)
                        .write_volatile(datum)
                };
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            }

            bao1x_api::bollard!(bao1x_hal::sigcheck::die_no_std, 4);
            self.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_WRITE_CMD | SECURITY_MODE);
            unsafe {
                self.array
                    .as_mut_ptr()
                    .add(addr / core::mem::size_of::<u32>() + outer * 8)
                    .write_volatile(RRC_LOAD_BUFFER);
            }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            unsafe {
                self.array
                    .as_mut_ptr()
                    .add(addr / core::mem::size_of::<u32>() + outer * 8)
                    .write_volatile(RRC_WRITE_BUFFER);
            }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            self.csr.rmwf(utra::rrc::SFR_RRCCR_SFR_RRCCR, RRC_CR_NORMAL | SECURITY_MODE);
        }
        bao1x_hal::cache_flush();
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
            let dest_slice = unsafe {
                core::slice::from_raw_parts(
                    (start_offset + utralib::HW_RERAM_MEM) as *const u8,
                    buffer.0.len(),
                )
            };
            // populate from old data first
            buffer.0.copy_from_slice(&dest_slice);
            for (dst, &src) in
                buffer.0[offset % ALIGNMENT..].iter_mut().zip(data[..start_len.min(data.len())].iter())
            {
                *dst = src;
            }
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
                    let dest_slice = unsafe {
                        core::slice::from_raw_parts(
                            (cur_offset + utralib::HW_RERAM_MEM) as *const u8,
                            buffer.0.len(),
                        )
                    };
                    // read in the destination full contents
                    buffer.0.copy_from_slice(&dest_slice);
                    // now overwrite the "ragged end"
                    buffer.0[..chunk.len()].copy_from_slice(&chunk);
                    // safe because alignment and buffer sizes are guaranteed
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

    fn write_slice_retry(&mut self, offset: usize, data: &[u8]) -> Result<usize, xous::Error> {
        for i in 0..ATTEMPTS {
            match self.write_slice_inner(offset, data) {
                Ok(len) => {
                    let check_slice = unsafe {
                        core::slice::from_raw_parts((offset + utralib::HW_RERAM_MEM) as *const u8, data.len())
                    };

                    if check_slice[..data.len().min(check_slice.len())]
                        == data[..data.len().min(check_slice.len())]
                    {
                        return Ok(len);
                    } else {
                        crate::println!("Write failed to verify retry {}/{}", i + 1, ATTEMPTS);
                    }
                }
                Err(e) => {
                    crate::println!("Write with error {:?}, retrying...", e);
                }
            }
        }
        Err(xous::Error::InternalError)
    }

    /// safety: absolutely no bounds checking on offset or data done prior to write. Mis-use of this
    /// function can brick the chip.
    pub unsafe fn crazy_unsafe_write_slice(
        &mut self,
        offset: usize,
        data: &[u8],
    ) -> Result<usize, xous::Error> {
        // leave these debug snippets, I think this will help with field diagnostics
        // even though they are unsightly.
        crate::println!("  Writing to: {:x}", offset);
        crate::println!("  Data {:x?}..{:x?}", &data[..16], &data[data.len() - 16..]);
        self.write_slice_retry(offset, data)
    }
}
