use utralib::generated::*;
use xous::MemoryRange;

const FB_WIDTH_WORDS: usize = 11;
const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
const CONFIG_CLOCK_FREQUENCY: u32 = 100_000_000;

const COMMAND_OFFSET: usize = 0;
const BUSY_OFFSET: usize = 1;
const PRESCALER_OFFSET: usize = 2;

pub struct XousDisplay {
    fb: MemoryRange,
    control: MemoryRange,
}

impl XousDisplay {
    pub fn new() -> XousDisplay {
        let fb = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_MEMLCD_MEM),
            None,
            ((FB_WIDTH_WORDS * FB_LINES * 4) + 4096) & !4095,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map frame buffer");
        for mem_offset in 0..(FB_SIZE / 4) {
            unsafe { fb.as_mut_ptr().add(mem_offset).write_volatile(0) };
        }

        let control = xous::syscall::map_memory(
            xous::MemoryAddress::new(HW_MEMLCD_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map control port");
        let mut display = XousDisplay { fb, control };

        display.set_clock(CONFIG_CLOCK_FREQUENCY);
        display.sync_clear();

        display
    }

    pub fn redraw(&mut self) {
        while self.busy() {}
        self.update_dirty();
    }

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
        unsafe {
            (self.control.as_ptr() as *mut u32)
                .add(PRESCALER_OFFSET)
                .write_volatile((clk_mhz / 2_000_000) - 1);
        }
    }

    fn update_all(&mut self) {
        unsafe {
            (self.control.as_ptr() as *mut u32)
                .add(COMMAND_OFFSET)
                .write_volatile(2)
        };
    }

    fn update_dirty(&mut self) {
        unsafe {
            (self.control.as_ptr() as *mut u32)
                .add(COMMAND_OFFSET)
                .write_volatile(1)
        };
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
        unsafe {
            (self.control.as_ptr() as *mut u32)
                .add(BUSY_OFFSET)
                .read_volatile()
                == 1
        }
    }
}
