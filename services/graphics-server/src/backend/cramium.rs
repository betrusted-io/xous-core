use cram_hal_service::IframRange;
use cramium_hal::iox;
use utralib::generated::*;
use utralib::utra;
use xous::MemoryRange;

/// # Display backend for Cramium target
///
/// There isn't a dedicated memory LCD frame buffer device, so it has to be
/// cobbled together somewhat painfully from a DMA engine and the PIO block.
///
/// The drawing frame buffer is laid out as 11 words by 536 lines.
///
/// Each line has 16 extra bits on the MSB of the last word that are used to
/// indicate if the line is dirty.
///
/// Drawing frame buffer:
/// 0x0000: [data 0, data 1, ... data 0A, data 0B | dirty << 16],
/// 0x000C: [data C, data D, ... data 15, data 16 | dirty << 16],
/// ...
///
/// The PIO is "dumb" in that it can only shift bits out; however, the memory
/// LCD requires addressing and mode set information on each line. In order
/// to accommodate this, the top 16 unused bits of the previous line is used
/// to encode the addressing and mode set information, referred to as `addr`
/// in the schematic below.
///
/// So, in the redraw routine, the drawing frame buffer is checked line-by-line
/// to see if the line is dirty; if it is, it is copied to the HW DMA buffer,
/// such that the line *prior* to the current line has the addressing bits
/// for the LCD inserted in the top 16 bits. This means that the first line
/// in the HW DMA buffer is mostly dummy words, plus the address of the first
/// line:
///
/// HW DMA buffer:
/// 0x0000: [dummy 0, dummy 1, .... dummy 0A, dummy 0B | addr 0 << 16],
/// 0x0030: [data  0,  data 1, .... data  0A, data  0B | addr 1 << 16],
/// ...
///
/// The HW DMA buffer will only contain the dirty lines to blit. Thus, when
/// passing the blit initiation command off to the `cram-hal-service` server,
/// the number of lines to blit must be specified.
///
/// There are two implementations currently contemplated: one using the PL230+PIO
/// block, and the other using the UDMA block.
///
/// ## PL230+PIO:
///
/// The PL230 DMA engine is configured to copy only the specified number
/// of half-words (where a half-word is 16 bits in length) into the PIO block,
/// computed as (23 half-words per line) * (number of lines), with the first
/// half-word originating at the addressing bits on the previous line. Thus the
/// DMA would start at address 0xA, shifting 16 bits of `addr 0` first, then
/// 22 16-bit words consisting of 352 bits of line data, and a final 16 bits of
/// "don't care" (which turns out to be the next line's address info).
///
/// The PIO engine is configured as a simple 16-bit, LSB-first shift register
/// to send data to the LCD at a rate of 2 MHz, requesting a new word via DMA
/// whenever the shift register is empty. Multi-line mode is used to drive
/// the LCD, so CS should be manually de-asserted at the conclusion of the transfer.
/// In this implementation, setup time on CS is enforced by the PIO with a short
/// pre-amble.
///
/// ## UDMA:
///
/// The UDMA engine is configured to send only the specified number of half-words
/// (where a half-word is 16 bits in length) into the SPIM block. I think the CS
/// line should still be managed through a GPIO bit-bang operation, since there
/// is a fairly long setup/hold time requirement on it and there does not seem
/// to be a provision in the UDMA block to put guardbands around the CS timing.

pub const FB_WIDTH_WORDS: usize = 11;
pub const FB_WIDTH_PIXELS: usize = WIDTH as usize;
pub const FB_LINES: usize = LINES as usize;
pub const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
const CONFIG_CLOCK_FREQUENCY: u32 = 100_000_000;

pub struct MainThreadToken(());

pub enum Never {}

#[inline]
pub fn claim_main_thread(f: impl FnOnce(MainThreadToken) -> Never + Send + 'static) -> ! {
    // Just call the closure - this backend will work on any thread
    #[allow(unreachable_code)] // false positive
    match f(MainThreadToken(())) {}
}

pub struct XousDisplay {
    fb: MemoryRange,
    hwfb: IframRange,
    next_free_line: usize,
    srfb: [u32; FB_SIZE],
    csr: utralib::CSR<u32>,
    devboot: bool,
}

impl XousDisplay {
    pub fn new(_main_thread_token: MainThreadToken) -> XousDisplay {
        let fb = xous::syscall::map_memory(
            None,
            None,
            ((FB_WIDTH_WORDS * FB_LINES * 4) + 4096) & !4095,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map backing frame buffer");
        // this is safe because all values of u32 are representable on the system
        unsafe {
            for w in fb.as_slice_mut() {
                *w = 0xFFFF_FFFFu32;
            }
        }

        // safety: this is safe because we bind hwfb to XousDisplay which lives for the entire
        // duration of the OS.
        let hwfb = unsafe {
            cram_hal_service::IframRange::request(4096 * 3, None)
                .expect("Couldn't allocate a DMA target for graphics!")
        };

        // setup the I/O pins
        let iox = cram_hal_service::IoxHal::new();
        // SPIM_CLK_B[1]
        iox.setup_io_pin(
            iox::IoxPort::PD,
            7,
            Some(iox::IoxDir::Output),
            Some(iox::IoxFunction::AF2),
            None,
            None,
            Some(iox::IoxEnable),
            Some(iox::IoxDriveStrength::Drive2mA),
        );
        // SPIM_SD0_B[1]
        iox.setup_io_pin(
            iox::IoxPort::PD,
            8,
            Some(iox::IoxDir::Output),
            Some(iox::IoxFunction::AF2),
            None,
            None,
            Some(iox::IoxEnable),
            Some(iox::IoxDriveStrength::Drive2mA),
        );
        // SPIM_SCSN0_B[1] - as GPIO
        iox.setup_io_pin(
            iox::IoxPort::PD,
            10,
            Some(iox::IoxDir::Output),
            Some(iox::IoxFunction::Gpio),
            None,
            Some(iox::IoxEnable),
            Some(iox::IoxEnable),
            Some(iox::IoxDriveStrength::Drive2mA),
        );

        let control = xous::syscall::map_memory(
            xous::MemoryAddress::new(utralib::generated::HW_UDMA_SPIM_1_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map control port");

        let mut display = XousDisplay {
            fb,
            hwfb,
            csr: CSR::new(control.as_mut_ptr() as *mut u32),
            srfb: [0u32; FB_SIZE],
            next_free_line: 0,
            devboot: bool,
        };

        display.set_clock(CONFIG_CLOCK_FREQUENCY);
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
    ///
    /// The addresses of these structures are passed as `u32` and unsafely cast back
    /// into pointers on the user's side. We do this because the panic handler is special:
    /// it grabs ahold of the low-level hardware, yanking control from the higher level
    /// control functons, without having to map its own separate pages.
    ///
    /// Of course, "anyone" with a copy of this data can clobber existing graphics operations. Thus,
    /// any access to these registers have to be protected with a mutex of some form. In the case of
    /// the panic handler, the `is_panic` `AtomicBool` will suppress graphics loop operation
    /// in the case of a panic.
    pub unsafe fn hw_regs(&self) -> (u32, u32) { (self.hwfb.as_mut_ptr() as u32, self.csr.base() as u32) }

    pub fn stash(&mut self) {
        unimplemented!("Cramium platform does not yet support suspend/resume");
    }

    pub fn pop(&mut self) {
        unimplemented!("Cramium platform does not yet support suspend/resume");
    }

    pub fn suspend(&mut self) {
        unimplemented!("Cramium platform does not yet support suspend/resume");
    }

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
        // this code is safe because u32 is representable on the system
        let fb = unsafe { self.fb.as_slice_mut::<u32>() };
        // check if a line is dirty; if so, copy it to the DMA buffer, then mark it as clean.
        for line_no in 0..FB_LINES {
            if fb[line_no * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] & 0xFFFF_0000 != 0x0 {
                self.copy_line_to_dma(line_no);
                fb[line_no * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
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
        let fb = unsafe { self.fb.as_slice_mut::<u32>() };
        // copy to the soft frame buffer
        fb.copy_from_slice(bmp);
        // now copy for DMA
        for line_no in 0..FB_LINES {
            self.copy_line_to_dma(line_no);
        }
        self.update_dirty();

        while self.busy() {}
    }

    pub fn as_slice(&self) -> &[u32] {
        // Safety: all values of `[u32]` are valid
        unsafe { &self.fb.as_slice::<u32>()[..FB_SIZE] }
    }

    /// Beneath this line are pure-HAL layer, and should not be user-visible
    /// Copies a display line to the DMA buffer, while setting up all the bits for
    /// the DMA operation. Manages the DMA line pointer as well.
    fn copy_line_to_dma(&self, src_line: usize) {
        // safety: this is safe because the underlying data types (u32) are representable on the system
        unsafe {
            let mut hwfb = self.hwfb.as_slice_u32_mut();
            let fb = self.fb.as_slice::<u32>();
            // set the mode and address
            // the very first line is unused, except for the mode & address info
            // this is done just to keep the math easy for computing strides & alignments
            hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS - 1] =
                (hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS - 1] & 0x0000_FFFFF)
                    | (src_line << 6)
                    | 0b001;
            // now copy the data
            hwfb[(self.next_free_line + 1) * FB_WIDTH_WORDS..(self.next_free_line + 2) * FB_WIDTH_WORDS]
                .copy_from_slice(&fb[src_line * FB_WIDTH_WORDS..(src_line + 1) * FB_WIDTH_WORDS]);
            if self.devboot && src_line == 7 {
                for w in hwfb
                    [(self.next_free_line + 1) * FB_WIDTH_WORDS..(self.next_free_line + 2) * FB_WIDTH_WORDS]
                    .iter_mut()
                {
                    *w = *w & 0xCCCC_CCCC; // hash over the status line
                }
            }

            self.next_free_line += 1;
        }
    }

    // NEXT UP: translate all the UDMA SPIM calls from the Pulp C source into the udma.rs file in cram-hal!
    fn set_clock(&mut self, clk_mhz: u32) {
        // TODO:
        //  - set up the UDMA block registers
    }

    fn update_dirty(&mut self) {
        // TODO:
        //  - set up DMA based on next_free_line

        self.next_free_line = 0;
    }

    fn busy(&self) -> bool {
        // TODO:
        //  - return if DMA is in progress by monitoring the UDMA bits
    }

    pub fn set_devboot(&mut self, ena: bool) {
        // one-way door (set-only)
        if ena {
            self.devboot = ena;
        }
    }
}
