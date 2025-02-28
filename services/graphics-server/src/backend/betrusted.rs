use susres::{RegManager, RegOrField, SuspendResume};
use utralib::generated::*;
use ux_api::minigfx::{ColorNative, FrameBuffer, PixelColor, Point};
use ux_api::platform::*;
use xous::MemoryRange;

const LCD_WORDS_PER_LINE: usize = FB_WIDTH_WORDS;
const LCD_PX_PER_LINE: isize = WIDTH as isize;
const LCD_LINES: isize = FB_LINES as isize;
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
    hwfb: MemoryRange,
    srfb: [u32; FB_SIZE],
    csr: utralib::CSR<u32>,
    susres: RegManager<{ utra::memlcd::MEMLCD_NUMREGS }>,
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
        let temp: *mut [u32; FB_SIZE] = fb.as_mut_ptr() as *mut [u32; FB_SIZE];
        for words in 0..FB_SIZE {
            unsafe {
                (*temp)[words] = 0xFFFF_FFFF;
            }
        }

        let hwfb = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_MEMLCD_MEM),
            None,
            ((FB_WIDTH_WORDS * FB_LINES * 4) + 4096) & !4095,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map hardware frame buffer");
        let temp: *mut [u32; FB_SIZE] = hwfb.as_mut_ptr() as *mut [u32; FB_SIZE];
        for words in 0..FB_SIZE {
            unsafe {
                (*temp)[words] = 0xFFFF_FFFF;
            }
        }

        let control = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_MEMLCD_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map control port");

        let mut display = XousDisplay {
            fb,
            hwfb,
            csr: CSR::new(control.as_mut_ptr() as *mut u32),
            susres: RegManager::new(control.as_mut_ptr() as *mut u32),
            srfb: [0u32; FB_SIZE],
        };

        display.set_clock(CONFIG_CLOCK_FREQUENCY);
        display.susres.push(RegOrField::Field(utra::memlcd::PRESCALER_PRESCALER), None);

        /*
        use log::{error, info};
        info!("GFX: fb 0x{:08x} bytes at 0x{:08x}", usize::from(fb.size), usize::from(fb.addr));
        info!("GFX: hwfb 0x{:08x} bytes at 0x{:08x}", usize::from(hwfb.size), usize::from(hwfb.addr));
        info!("GFX: csr 0x{:08x} bytes at 0x{:08x}", usize::from(control.size), usize::from(control.addr));
        */

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
    /// control functions, without having to map its own separate pages.
    ///
    /// Of course, "anyone" with a copy of this data can clobber existing graphics operations. Thus,
    /// any access to these registers have to be protected with a mutex of some form. In the case of
    /// the panic handler, the `is_panic` `AtomicBool` will suppress graphics loop operation
    /// in the case of a panic.
    pub unsafe fn hw_regs(&self) -> (u32, u32) { (self.hwfb.as_mut_ptr() as u32, self.csr.base() as u32) }

    pub fn stash(&mut self) {
        let fb = unsafe { core::slice::from_raw_parts_mut(self.fb.as_mut_ptr() as *mut u32, FB_SIZE) };
        for lines in 0..FB_LINES {
            // set the dirty bits prior to stashing the frame buffer
            fb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
        }
        let srfb_ptr = self.srfb.as_mut_ptr() as *mut u32;
        for (index, &src) in fb.iter().enumerate() {
            unsafe { srfb_ptr.add(index).write_volatile(src) };
        }
    }

    pub fn pop(&mut self) {
        // Safety: `u32` contains no undefined values
        let fb: &mut [u32] = unsafe { self.fb.as_slice_mut() };
        // skip copying the status bar, so that the status info is not overwritten by the pop.
        // this is "fixed" at 32 pixels high (2 * Cjk glyph height hint) per line 79 in gam/src/main.rs
        fb[FB_WIDTH_WORDS * 32..FB_SIZE].copy_from_slice(&self.srfb[FB_WIDTH_WORDS * 32..FB_SIZE]);
        self.redraw();
    }

    pub fn suspend(&mut self) {
        while self.busy() {
            // just wait until any pending FB operations are done
        }
        let fb = unsafe { core::slice::from_raw_parts_mut(self.fb.as_mut_ptr() as *mut u32, FB_SIZE) };
        for lines in 0..FB_LINES {
            // set the dirty bits prior to stashing the frame buffer
            fb[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
        }
        let srfb_ptr = self.srfb.as_mut_ptr() as *mut u32;
        for (index, &src) in fb.iter().enumerate() {
            unsafe { srfb_ptr.add(index).write_volatile(src) };
        }
        self.susres.suspend();

        let note = crate::sleep_note::LOGO_MAP;
        let note_lines = note.len() / FB_WIDTH_WORDS;
        let note_start_line = (FB_LINES - note_lines) / 2;
        let note_end_line = note_start_line + note_lines;
        let hwfb: *mut [u32; FB_SIZE] = self.hwfb.as_mut_ptr() as *mut [u32; FB_SIZE];

        for lines in 0..FB_LINES {
            for words in 0..FB_WIDTH_WORDS {
                if lines >= note_start_line && lines < note_end_line {
                    // We're on a line that has the 'Sleeping...' note.
                    // Draw it into the frame buffer.
                    let note_index = ((lines - note_start_line) * FB_WIDTH_WORDS) + words;
                    unsafe {
                        (*hwfb)[words + lines * FB_WIDTH_WORDS] = note[note_index];
                    }
                } else {
                    // We're not on a line that has the note. Clear the pixels,
                    // since we don't want any secrets to linger in the framebuffer
                    // and stay visible on the persistent display while the device is
                    // sleeping.
                    unsafe {
                        (*hwfb)[words + lines * FB_WIDTH_WORDS] = 0xFFFF_FFFF;
                    }
                }
            }

            // set the dirty bits
            unsafe {
                (*hwfb)[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] |= 0x1_0000;
            }
        }

        self.update_dirty();
        while self.busy() {
            // busy wait, blocking suspend until this has happened
        }
    }

    pub fn resume(&mut self) {
        self.susres.resume();
        // Safety: `u32` contains no undefined values
        let fb: &mut [u32] = unsafe { self.fb.as_slice_mut() };
        fb[..FB_SIZE].copy_from_slice(&self.srfb);

        self.redraw();
    }

    pub fn screen_size(&self) -> Point { Point::new(FB_WIDTH_PIXELS as isize, FB_LINES as isize) }

    pub fn redraw(&mut self) {
        let mut busy_count = 0;
        let mut dirty_count = 0;
        while self.busy() {
            xous::yield_slice();
            busy_count += 1;
        }
        let fb: *mut [u32; FB_SIZE] = self.fb.as_mut_ptr() as *mut [u32; FB_SIZE];
        let hwfb: *mut [u32; FB_SIZE] = self.hwfb.as_mut_ptr() as *mut [u32; FB_SIZE];
        for words in 0..FB_SIZE {
            unsafe {
                (*hwfb)[words] = (*fb)[words];
            }
        }
        self.update_dirty();
        // clear all the dirty bits, under the theory that it's time-wise cheaper on average
        // to visit every line and clear the dirty bits than it is to do an update_all()
        for lines in 0..FB_LINES {
            if unsafe { (*fb)[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] & 0xFFFF_0000 } != 0x0 {
                dirty_count += 1;
            }
            unsafe {
                (*fb)[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
            }
        }
        log::trace!("redraw {}/{}", busy_count, dirty_count);
    }

    pub fn native_buffer(&mut self) -> &mut [u32; FB_SIZE] {
        unsafe { &mut *(self.fb.as_mut_ptr() as *mut [u32; FB_SIZE]) }
    }

    pub fn blit_screen(&mut self, bmp: &[u32]) {
        let framebuffer = self.fb.as_mut_ptr() as *mut u32;

        for words in 0..FB_SIZE {
            unsafe {
                framebuffer.add(words).write_volatile(bmp[words]);
            }
        }
        self.update_all();

        while self.busy() {}
    }

    pub fn as_slice(&self) -> &[u32] {
        // Safety: all values of `[u32]` are valid
        unsafe { &self.fb.as_slice::<u32>()[..FB_SIZE] }
    }

    /// Beneath this line are pure-HAL layer, and should not be user-visible

    ///
    fn set_clock(&mut self, clk_mhz: u32) {
        self.csr.wfo(utra::memlcd::PRESCALER_PRESCALER, (clk_mhz / 2_000_000) - 1);
    }

    fn update_all(&mut self) { self.csr.wfo(utra::memlcd::COMMAND_UPDATEALL, 1); }

    fn update_dirty(&mut self) { self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1); }

    /// "synchronous clear" -- must be called on init, so that the state of the LCD
    /// internal memory is consistent with the state of the frame buffer
    /*
    fn sync_clear(&mut self) {
        let framebuffer = self.fb.as_mut_ptr() as *mut u32;
        for words in 0..FB_SIZE {
            if words % FB_WIDTH_WORDS != 10 {
                unsafe { framebuffer.add(words).write_volatile(0xFFFF_FFFF) };
            } else {
                unsafe { framebuffer.add(words).write_volatile(0x0000_FFFF) };
            }
        }
        self.update_all(); // because we force an all update here
        while self.busy() {}
    }
    */

    fn busy(&self) -> bool { self.csr.rf(utra::memlcd::BUSY_BUSY) == 1 }

    pub fn set_devboot(&mut self, ena: bool) {
        if ena {
            self.csr.wfo(utra::memlcd::DEVBOOT_DEVBOOT, 1);
        } else {
            self.csr.wfo(utra::memlcd::DEVBOOT_DEVBOOT, 0);
        }
    }
}

impl FrameBuffer for XousDisplay {
    /// Puts a pixel of ColorNative at x, y. (0, 0) is defined as the lower left corner.
    fn put_pixel(&mut self, p: Point, color: ColorNative) {
        let fb: &mut [u32] = unsafe { self.fb.as_slice_mut() };
        let clip_y: usize;
        if p.y >= LCD_LINES {
            clip_y = LCD_LINES as usize - 1;
        } else if p.y < 0 {
            clip_y = 0;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            clip_x = LCD_PX_PER_LINE as usize - 1;
        } else if p.x < 0 {
            clip_x = 0;
        } else {
            clip_x = p.x as usize;
        }

        let pc = PixelColor::from(color.0);

        if pc == PixelColor::Light {
            fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] |= 1 << (clip_x % 32)
        } else {
            fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] &= !(1 << (clip_x % 32))
        }
        // set the dirty bit on the line that contains the pixel
        fb[clip_y * LCD_WORDS_PER_LINE + (LCD_WORDS_PER_LINE - 1)] |= 0x1_0000;
    }

    /// Retrieves a pixel value from the frame buffer; returns None if the point is out of bounds.
    ///
    /// Note: this has not be carefully tested as this API is not used by the legacy code base.
    /// Anyone using this API for the first time may benefit in checking that it is correct.
    fn get_pixel(&self, p: Point) -> Option<ColorNative> {
        let fb: &[u32] = unsafe { self.fb.as_slice() };
        let clip_y: usize;
        if p.y >= LCD_LINES {
            return None;
        } else if p.y < 0 {
            return None;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            return None;
        } else if p.x < 0 {
            return None;
        } else {
            clip_x = p.x as usize;
        }
        Some(ColorNative::from(
            (fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] & (1 << (clip_x % 32))) as usize,
        ));
        todo!("Check that this API is implemented correctly before using it!");
    }

    /// XORs a pixel to what is in the existing frame buffer. The exact definition of "XOR" is somewhat
    /// ambiguous for full color systems but is generally meant to imply a light/dark swap of foreground
    /// and background colors for a color theme.
    fn xor_pixel(&mut self, p: Point) {
        let fb: &mut [u32] = unsafe { self.fb.as_slice_mut() };
        let clip_y: usize;
        if p.y >= LCD_LINES {
            return;
        } else if p.y < 0 {
            return;
        } else {
            clip_y = p.y as usize;
        }

        let clip_x: usize;
        if p.x >= LCD_PX_PER_LINE {
            return;
        } else if p.x < 0 {
            return;
        } else {
            clip_x = p.x as usize;
        }

        fb[(clip_x + clip_y * LCD_WORDS_PER_LINE * 32) / 32] ^= 1 << (clip_x % 32);
        // set the dirty bit on the line that contains the pixel
        fb[clip_y * LCD_WORDS_PER_LINE + (LCD_WORDS_PER_LINE - 1)] |= 0x1_0000;
    }

    /// Swaps the drawable buffer to the screen and sends it to the hardware
    fn draw(&mut self) { self.redraw(); }

    /// Clears the drawable buffer
    fn clear(&mut self) {
        let fb: &mut [u32] = unsafe { self.fb.as_slice_mut() };
        fb.fill(0xFFFF_FFFF);
    }

    /// Returns the size of the frame buffer as a Point
    fn dimensions(&self) -> Point { self.screen_size() }

    /// Returns a raw pointer to the frame buffer
    unsafe fn raw_mut(&mut self) -> &mut ux_api::platform::FbRaw { self.native_buffer() }
}
