use embedded_graphics::{drawable::Pixel, geometry::Size, pixelcolor::BinaryColor, DrawTarget};

const FB_WIDTH_WORDS: usize = 11;
const FB_WIDTH_PIXELS: usize = 336;
const FB_LINES: usize = 536;
const FB_SIZE: usize = FB_WIDTH_WORDS * FB_LINES; // 44 bytes by 536 lines
const CONFIG_CLOCK_FREQUENCY: u32 = 100_000_000;

// const MAX_FPS: u64 = 15;

const COMMAND_OFFSET: usize = 0;
const BUSY_OFFSET: usize = 1;
const PRESCALER_OFFSET: usize = 2;

pub struct XousDisplay {
    fb: &'static mut [u32; FB_WIDTH_PIXELS * FB_LINES],
    control: *mut u32,
}

impl XousDisplay {
    pub fn new() -> XousDisplay {
        let fb = unsafe { &mut *(0xb000_0000 as *mut [u32; FB_WIDTH_PIXELS * FB_LINES]) };
        let mut display = XousDisplay {
            fb,
            control: 0xf000_a000u32 as _,
        };
        display.set_clock(CONFIG_CLOCK_FREQUENCY);
        display.sync_clear();
        display
    }

    pub fn redraw(&mut self) {
        while self.busy() {}
        self.update_dirty();
    }

    pub fn update(&mut self) {}

    /// Beneath this line are pure-HAL layer, and should not be user-visible

    ///
    fn set_clock(&mut self, clk_mhz: u32) {
        unsafe {
            self.control
                .add(PRESCALER_OFFSET)
                .write_volatile((clk_mhz / 2_000_000) - 1);
        }
    }

    fn update_all(&mut self) {
        unsafe { self.control.add(COMMAND_OFFSET).write_volatile(2) };
    }

    fn update_dirty(&mut self) {
        unsafe { self.control.add(COMMAND_OFFSET).write_volatile(1) };
    }

    /// "synchronous clear" -- must be called on init, so that the state of the LCD
    /// internal memory is consistent with the state of the frame buffer
    fn sync_clear(&mut self) {
        for words in 0..FB_SIZE {
            if words % FB_WIDTH_WORDS != 10 {
                self.fb[words] = 0xFFFF_FFFF;
            } else {
                self.fb[words] = 0x0000_FFFF;
            }
        }
        self.update_all(); // because we force an all update here
        while self.busy() {}
    }

    fn busy(&self) -> bool {
        unsafe { self.control.add(BUSY_OFFSET).read_volatile() == 1 }
    }
}

impl DrawTarget<BinaryColor> for XousDisplay {
    type Error = core::convert::Infallible;

    /// Draw a `Pixel` that has a color defined as `Gray8`.
    fn draw_pixel(&mut self, pixel: Pixel<BinaryColor>) -> Result<(), Self::Error> {
        let Pixel(coord, color) = pixel;
        match color {
            BinaryColor::Off => {
                self.fb[(coord.x / 32 + coord.y * FB_WIDTH_WORDS as i32) as usize] |=
                    1 << (coord.x % 32)
            }
            BinaryColor::On => {
                self.fb[(coord.x / 32 + coord.y * FB_WIDTH_WORDS as i32) as usize] &=
                    !(1 << (coord.x % 32))
            }
        }
        // set the dirty bit on the line
        self.fb[(coord.y * FB_WIDTH_WORDS as i32 + (FB_WIDTH_WORDS as i32 - 1)) as usize] |=
            0x1_0000;

        // self.buffer[(point.y * (WIDTH as i32) + point.x) as usize] = if color == BinaryColor::On {
        //     LIGHT_COLOUR
        // } else {
        //     DARK_COLOUR
        // };
        Ok(())
    }

    fn size(&self) -> Size {
        Size::new(FB_WIDTH_PIXELS as _, FB_LINES as _)
    }
}
