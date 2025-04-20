use cramium_api::camera;
use cramium_api::*;

const EMU_COLS: usize = 320;
const EMU_ROWS: usize = 240;

#[repr(C)]
pub struct Ov2640 {
    frame: [u16; EMU_COLS * EMU_ROWS],
}

impl Ov2640 {
    /// Safety: clocks must be turned on before this is called
    pub unsafe fn new() -> Result<Self, xous::Error> { Ok(Self { frame: [0u16; EMU_COLS * EMU_ROWS] }) }

    pub fn release_ifram(&mut self) {}

    pub fn claim_ifram(&mut self) -> Result<(), xous::Error> { Ok(()) }

    pub fn has_ifram(&self) -> bool { false }

    pub fn poke(&self, _i2c: &mut dyn I2cApi, _adr: u8, _dat: u8) {}

    // chip does not support sequential reads
    pub fn peek(&self, _i2c: &mut dyn I2cApi, _adr: u8, _dat: &mut [u8]) {}

    pub fn init(&mut self, _i2c: &mut dyn I2cApi, _resolution: camera::Resolution) {}

    /// Safety: definitely not safe, but only used in emulation mode so I'm not going to worry about it too
    /// much.
    pub fn rx_buf<T: UdmaWidths>(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                self.frame.as_ptr() as *const T,
                self.frame.len() / core::mem::size_of::<T>(),
            )
        }
    }

    /// Safety: definitely not safe, but only used in emulation mode so I'm not going to worry about it too
    /// much.
    pub unsafe fn rx_buf_phys<T: UdmaWidths>(&self) -> &[T] {
        unsafe {
            core::slice::from_raw_parts(
                self.frame.as_ptr() as *const T,
                self.frame.len() / core::mem::size_of::<T>(),
            )
        }
    }

    pub fn capture_async(&mut self) {}

    pub fn capture_await(&mut self, _use_yield: bool) {}

    /// For emulation, we fix the resolution at 320x240
    pub fn resolution(&self) -> (usize, usize) { (EMU_COLS, EMU_ROWS) }

    pub fn set_slicing(&mut self, _ll: (usize, usize), _ur: (usize, usize)) {}

    pub fn disable_slicing(&mut self) {}

    // Commented out to avoid leaking the ColorMode type
    // pub fn color_mode(&mut self, _i2c: &mut dyn I2cApi, _mode: ColorMode) {}

    // Commented out to avoid leaking the Contrast & Brightness types
    /*
    pub fn contrast_brightness(
        &mut self,
        _i2c: &mut dyn I2cApi,
        _contrast: Contrast,
        _brightness: Brightness,
    ) {
    }
    */

    // Commented out to avoid leaking the Effect types
    // pub fn effect(&mut self, _i2c: &mut dyn I2cApi, _effect: Effect) {}

    /// Returns (product ID, manufacturer ID) - hard coded to match OV2640 values
    pub fn read_id(&self, _i2c: &mut dyn I2cApi) -> (u16, u16) { (0x2641, 0x7fa2) }

    pub fn delay(&self, quantum: usize) {
        std::thread::sleep(std::time::Duration::from_millis(quantum as u64));
    }
}
