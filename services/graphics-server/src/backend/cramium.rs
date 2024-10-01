//! # Display backend for Cramium target
//!
//! There isn't a dedicated memory LCD frame buffer device, so it has to be
//! cobbled together somewhat painfully from a DMA engine and the PIO block.
//!
//! The drawing frame buffer is laid out as 11 words by 536 lines.
//!
//! Each line has 16 extra bits on the MSB of the last word that are used to
//! indicate if the line is dirty.
//!
//! Drawing frame buffer:
//! 0x0000: [data 0, data 1, ... data 0A, data 0B | dirty << 16],
//! 0x000C: [data C, data D, ... data 15, data 16 | dirty << 16],
//! ...
//!
//! The PIO is "dumb" in that it can only shift bits out; however, the memory
//! LCD requires addressing and mode set information on each line. In order
//! to accommodate this, the top 16 unused bits of the previous line is used
//! to encode the addressing and mode set information, referred to as `addr`
//! in the schematic below.
//!
//! So, in the redraw routine, the drawing frame buffer is checked line-by-line
//! to see if the line is dirty; if it is, it is copied to the HW DMA buffer,
//! such that the line *prior* to the current line has the addressing bits
//! for the LCD inserted in the top 16 bits. This means that the first line
//! in the HW DMA buffer is mostly dummy words, plus the address of the first
//! line:
//!
//! HW DMA buffer:
//! 0x0000: [dummy 0, dummy 1, .... dummy 0A, dummy 0B | addr 0 << 16],
//! 0x0030: [data  0,  data 1, .... data  0A, data  0B | addr 1 << 16],
//! ...
//!
//! The HW DMA buffer will only contain the dirty lines to blit. Thus, when
//! passing the blit initiation command off to the `cram-hal-service` server,
//! the number of lines to blit must be specified.
//!
//! There are two implementations currently contemplated: one using the PL230+PIO
//! block, and the other using the UDMA block.
//!
//! ## PL230+PIO:
//!
//! The PL230 DMA engine is configured to copy only the specified number
//! of half-words (where a half-word is 16 bits in length) into the PIO block,
//! computed as (23 half-words per line) * (number of lines), with the first
//! half-word originating at the addressing bits on the previous line. Thus the
//! DMA would start at address 0xA, shifting 16 bits of `addr 0` first, then
//! 22 16-bit words consisting of 352 bits of line data, and a final 16 bits of
//! "don't care" (which turns out to be the next line's address info).
//!
//! The PIO engine is configured as a simple 16-bit, LSB-first shift register
//! to send data to the LCD at a rate of 2 MHz, requesting a new word via DMA
//! whenever the shift register is empty. Multi-line mode is used to drive
//! the LCD, so CS should be manually de-asserted at the conclusion of the transfer.
//! In this implementation, setup time on CS is enforced by the PIO with a short
//! pre-amble.
//!
//! ## UDMA:
//!
//! The UDMA engine is configured to send only the specified number of half-words
//! (where a half-word is 16 bits in length) into the SPIM block. I think the CS
//! line should still be managed through a GPIO bit-bang operation, since there
//! is a fairly long setup/hold time requirement on it and there does not seem
//! to be a provision in the UDMA block to put guardbands around the CS timing.

use core::mem::size_of;

use cram_hal_service::IoxHal;
use cramium_hal::board::setup_display_pins;
use cramium_hal::udma;
use cramium_hal::udma::PeriphId;

use crate::api::Point;
use crate::api::{LINES, WIDTH};

pub const FB_WIDTH_WORDS: usize = 11;
pub const FB_WIDTH_PIXELS: usize = WIDTH as usize;
pub const FB_LINES: usize = LINES as usize;
pub const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines

const CONFIG_CLOCK_FREQUENCY: u32 = 50_000_000;

pub struct MainThreadToken(());

pub enum Never {}

#[inline]
pub fn claim_main_thread(f: impl FnOnce(MainThreadToken) -> Never + Send + 'static) -> ! {
    // Just call the closure - this backend will work on any thread
    #[allow(unreachable_code)] // false positive
    match f(MainThreadToken(())) {}
}

pub struct XousDisplay {
    fb: [u32; FB_SIZE],
    srfb: [u32; FB_SIZE],
    next_free_line: usize,
    spim: udma::Spim,
    devboot: bool,
}

impl XousDisplay {
    pub fn new(_main_thread_token: MainThreadToken) -> XousDisplay {
        let udma_global = cram_hal_service::UdmaGlobal::new();

        // setup the I/O pins
        let iox = IoxHal::new();
        let channel = setup_display_pins(&iox);
        udma_global.udma_clock_config(PeriphId::from(channel), true);

        log::info!("gfx using spi channel {:?}", channel);
        // safety: this is safe because we remembered to set up the clock config; and,
        // this binding should live for the lifetime of Xous so we don't have to worry about unmapping.
        let spim = unsafe {
            cramium_hal::udma::Spim::new(
                channel,
                2_000_000,
                CONFIG_CLOCK_FREQUENCY,
                udma::SpimClkPol::LeadingEdgeRise,
                udma::SpimClkPha::CaptureOnLeading,
                udma::SpimCs::Cs0,
                3,
                2,
                None,
                // one extra line for handling the addressing setup
                (FB_LINES + 1) * FB_WIDTH_WORDS * size_of::<u32>(),
                0,
                None,
            )
            .expect("Couldn't allocate SPI channel for LCD")
        };

        let mut display = XousDisplay {
            fb: [0xFFFF_FFFFu32; FB_SIZE],
            srfb: [0xFFFF_FFFFu32; FB_SIZE],
            spim,
            next_free_line: 0,
            devboot: false,
        };

        // initialize the DMA buffer with valid mode/address lines & blank data
        for line in 0..FB_LINES {
            display.copy_line_to_dma(line)
        }
        // but don't blit the data -- reset the pointer back to 0.
        display.next_free_line = 0;

        display
    }

    /// This should only be called to initialize the panic handler with its own
    /// copy of hardware registers.
    ///
    /// # Safety
    /// This creates a raw copy of the SPI hardware handle, which diverges from the copy used
    /// by the framebuffer. This is only safe when there are no more operations to be done to
    /// adjust the hardware mode of the SPIM.
    ///
    /// Furthermore, "anyone" with a copy of this data can clobber existing graphics operations. Thus,
    /// any access to these registers have to be protected with a mutex of some form. In the case of
    /// the panic handler, the `is_panic` `AtomicBool` will suppress graphics loop operation
    /// in the case of a panic.
    pub unsafe fn hw_regs(
        &self,
    ) -> (
        usize,
        udma::SpimCs,
        u8,
        u8,
        Option<udma::EventChannel>,
        udma::SpimMode,
        udma::SpimByteAlign,
        cramium_hal::ifram::IframRange,
        usize,
        usize,
        u8,
    ) {
        self.spim.into_raw_parts()
    }

    pub fn stash(&mut self) {
        self.srfb.copy_from_slice(&self.fb);

        for lines in 0..FB_LINES {
            // set the dirty bits prior to stashing the frame buffer
            self.srfb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
        }
    }

    pub fn pop(&mut self) {
        // skip copying the status bar, so that the status info is not overwritten by the pop.
        // this is "fixed" at 34 pixels high (2 * Tall glyph height hint) per line 79 in gam/src/main.rs
        self.fb[FB_WIDTH_WORDS * 34..FB_SIZE].copy_from_slice(&self.srfb[FB_WIDTH_WORDS * 34..FB_SIZE]);
        self.redraw();
    }

    #[allow(dead_code)]
    pub fn suspend(&mut self) {
        unimplemented!("Cramium platform does not yet support suspend/resume");
    }

    #[allow(dead_code)]
    pub fn resume(&mut self) {
        unimplemented!("Cramium platform does not yet support suspend/resume");
    }

    pub fn screen_size(&self) -> Point { Point::new(FB_WIDTH_PIXELS as i16, FB_LINES as i16) }

    pub fn redraw(&mut self) {
        let mut busy_count = 0;
        let mut dirty_count = 0;
        while self.busy() {
            xous::yield_slice();
            busy_count += 1;
        }
        // check if a line is dirty; if so, copy it to the DMA buffer, then mark it as clean.
        for line_no in 0..FB_LINES {
            // check an immutably borrowed copy of the soft framebuffer to see if the line is dirty,
            // and store the result.
            let is_dirty = if self.fb[line_no * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] & 0xFFFF_0000 != 0x0 {
                true
            } else {
                false
            };
            // dirty check is split from the line update to avoid re-borrowing the immutable borrow that was
            // needed to check the dirty state.
            if is_dirty {
                // this borrows self to copy the line data to the DMA buffer
                self.copy_line_to_dma(line_no);
                // this borrows self.fb to clear the dirty flag on the soft framebuffer
                // this code is safe because u32 is representable on the system
                self.fb[line_no * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
                dirty_count += 1;
            }
        }
        self.update_dirty();
        log::trace!("redraw {}/{}", busy_count, dirty_count);
    }

    pub fn native_buffer(&mut self) -> &mut [u32; FB_SIZE] {
        unsafe { &mut *(self.fb.as_mut_ptr() as *mut [u32; FB_SIZE]) }
    }

    pub fn blit_screen(&mut self, bmp: &[u32]) {
        // this code is safe because u32 is representable on the system
        // copy to the soft frame buffer
        self.fb[..bmp.len()].copy_from_slice(bmp);
        // now copy for DMA
        for line_no in 0..FB_LINES {
            self.copy_line_to_dma(line_no);
        }
        self.update_dirty();

        while self.busy() {}
    }

    pub fn as_slice(&self) -> &[u32] {
        // Safety: all values of `[u32]` are valid
        &self.fb[..FB_SIZE]
    }

    /// Beneath this line are pure-HAL layer, and should not be user-visible
    /// Copies a display line to the DMA buffer, while setting up all the bits for
    /// the DMA operation. Manages the DMA line pointer as well.
    fn copy_line_to_dma(&mut self, src_line: usize) {
        let hwfb = self.spim.tx_buf_mut();
        // safety: this is safe because `u32` has no invalid values
        // set the mode and address
        // the very first line is unused, except for the mode & address info
        // this is done just to keep the math easy for computing strides & alignments
        hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS - 1] =
            (hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS - 1] & 0x0000_FFFF)
                | (((src_line as u32) << 6) | 0b001) << 16;
        // now copy the data
        hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS..(self.next_free_line + 2) * FB_WIDTH_WORDS]
            .copy_from_slice(&self.fb[src_line * FB_WIDTH_WORDS..(src_line + 1) * FB_WIDTH_WORDS]);
        if self.devboot && src_line == 7 {
            for w in hwfb
                [(self.next_free_line + 1) * FB_WIDTH_WORDS..(self.next_free_line + 2) * FB_WIDTH_WORDS]
                .iter_mut()
            {
                *w = *w & 0xCCCC_CCCC; // hash over the status line
            }
        }

        if self.next_free_line < LINES as usize {
            self.next_free_line += 1;
        } else {
            log::warn!(
                "Line overflow in DMA detected. Suspect missing `update_dirty` call. Further lines will overwrite the last line."
            );
        }
    }

    fn update_dirty(&mut self) {
        if self.next_free_line != 0 {
            // safety: this function is safe to call because:
            //   - `is_virtual` is `false` => data should be a physical buffer that is pre-populated with the
            //     transmit data this is done by `copy_line_to_dma()`
            //   - the `data` argument is a physical buffer slice, which is only used as a base/bounds
            //     argument
            unsafe {
                self.spim.tx_data_async_from_parts::<u16>(
                    FB_WIDTH_WORDS * 2 - 1,
                    // +1 for the trailing dummy bits
                    self.next_free_line * FB_WIDTH_WORDS * 2 + 1,
                    true,
                    false,
                );
            }
            self.next_free_line = 0;
        }
    }

    fn busy(&self) -> bool { self.spim.is_tx_busy() }

    pub fn set_devboot(&mut self, ena: bool) {
        // one-way door (set-only)
        if ena {
            self.devboot = ena;
        }
    }
}
