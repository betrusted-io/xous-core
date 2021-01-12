use utralib::generated::*;
use xous::MemoryRange;

const FB_WIDTH_WORDS: usize = 11;
// const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
const CONFIG_CLOCK_FREQUENCY: u32 = 100_000_000;

pub struct XousDisplay {
    fb: MemoryRange,
    hwfb: MemoryRange,
    csr: utralib::CSR<u32>,
}

impl XousDisplay {
    pub fn new() -> XousDisplay {
        let fb = xous::syscall::map_memory(
            None,
            None,
            ((FB_WIDTH_WORDS * FB_LINES * 4) + 4096) & !4095,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map backing frame buffer");
        let temp: *mut [u32; FB_SIZE] = fb.as_mut_ptr() as *mut [u32; FB_SIZE];
        for words in 0..FB_SIZE {
            unsafe{(*temp)[words] = 0xFFFF_FFFF;}
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
            unsafe{(*temp)[words] = 0xFFFF_FFFF;}
        }

        let control = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_MEMLCD_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map control port");

        let mut display = XousDisplay {
            fb: fb,
            hwfb: hwfb,
            csr: CSR::new(control.as_mut_ptr() as *mut u32),
         };

        display.set_clock(CONFIG_CLOCK_FREQUENCY);
        display.sync_clear();

        display
    }

    pub fn redraw(&mut self) {
        while self.busy() {xous::yield_slice()}
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
            unsafe {
                (*fb)[lines * FB_WIDTH_WORDS + (FB_WIDTH_WORDS - 1)] &= 0x0000_FFFF;
            }
        }
    }

    // note: this API is used by emulation, don't remove calls to it
    pub fn update(&mut self) {}

    pub fn native_buffer(&mut self) -> &mut [u32; FB_SIZE] {
        unsafe { &mut *(self.fb.as_mut_ptr() as *mut [u32; FB_SIZE]) }
    }

    pub fn blit_screen(&mut self, bmp: [u32; FB_SIZE]) {
        let framebuffer = self.fb.as_mut_ptr() as *mut u32;

        for words in 0..FB_SIZE {
            unsafe {
                framebuffer.add(words).write_volatile(bmp[words]);
            }
        }
        self.update_all();

        while self.busy() {}
    }

    /// Beneath this line are pure-HAL layer, and should not be user-visible

    ///
    fn set_clock(&mut self, clk_mhz: u32) {
        self.csr.wfo(utra::memlcd::PRESCALER_PRESCALER, (clk_mhz / 2_000_000) - 1);
    }

    fn update_all(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEALL, 1);
    }

    fn update_dirty(&mut self) {
        self.csr.wfo(utra::memlcd::COMMAND_UPDATEDIRTY, 1);
    }

    /// "synchronous clear" -- must be called on init, so that the state of the LCD
    /// internal memory is consistent with the state of the frame buffer
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

    fn busy(&self) -> bool {
        self.csr.rf(utra::memlcd::BUSY_BUSY) == 1
    }
}
