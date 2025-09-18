use bao1x_api::camera::*;
use bao1x_api::*;
use utralib::CSR;
use utralib::utra;
use utralib::utra::udma_camera::REG_CAM_CFG_GLOB;

use super::constants::*;
use crate::ifram::IframRange;
use crate::udma::Udma;
use crate::udma::*;

pub const OV2640_DEV: u8 = 0x30;

pub const CFG_FRAMEDROP_EN: utralib::Field = utralib::Field::new(1, 0, REG_CAM_CFG_GLOB);
pub const CFG_FRAMEDROP_VAL: utralib::Field = utralib::Field::new(6, 1, REG_CAM_CFG_GLOB);
pub const CFG_FRAMESLICE_EN: utralib::Field = utralib::Field::new(1, 7, REG_CAM_CFG_GLOB);
pub const CFG_FORMAT: utralib::Field = utralib::Field::new(3, 8, REG_CAM_CFG_GLOB);
pub const CFG_SHIFT: utralib::Field = utralib::Field::new(4, 11, REG_CAM_CFG_GLOB);
pub const CFG_GLOB_EN: utralib::Field = utralib::Field::new(1, 31, REG_CAM_CFG_GLOB);

pub struct Ov2640 {
    csr: CSR<u32>,
    ifram: Option<IframRange>,
    contrast: Contrast,
    brightness: Brightness,
    color_mode: ColorMode,
    resolution: Resolution,
    slicing: Option<(usize, usize)>,
}

impl Udma for Ov2640 {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}

impl Ov2640 {
    #[cfg(feature = "std")]
    /// Safety: clocks must be turned on before this is called
    pub unsafe fn new() -> Result<Self, xous::Error> {
        let ifram_virt = xous::syscall::map_memory(
            xous::MemoryAddress::new(crate::board::CAM_IFRAM_ADDR),
            None,
            crate::board::CAM_IFRAM_LEN_PAGES * 4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )?;
        let ifram = IframRange::from_raw_parts(
            crate::board::CAM_IFRAM_ADDR,
            ifram_virt.as_ptr() as usize,
            ifram_virt.len(),
        );
        Ok(Ov2640::new_with_ifram(ifram))
    }

    pub unsafe fn new_with_ifram(ifram: IframRange) -> Self {
        #[cfg(target_os = "xous")]
        let csr_range = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::udma_camera::HW_UDMA_CAMERA_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map cam port");
        #[cfg(target_os = "xous")]
        let csr = CSR::new(csr_range.as_mut_ptr() as *mut u32);
        #[cfg(not(target_os = "xous"))]
        let csr = CSR::new(utra::udma_camera::HW_UDMA_CAMERA_BASE as *mut u32);

        Self {
            csr,
            ifram: Some(ifram),
            color_mode: ColorMode::Normal,
            contrast: Contrast::Level2,
            brightness: Brightness::Level2,
            // bogus value
            resolution: Resolution::Res160x120,
            slicing: None,
        }
    }

    pub fn release_ifram(&mut self) {
        if let Some(ifram) = self.ifram.take() {
            xous::syscall::unmap_memory(ifram.virt_range).unwrap();
        }
    }

    pub fn claim_ifram(&mut self) -> Result<(), xous::Error> {
        if self.ifram.is_none() {
            let ifram = xous::syscall::map_memory(
                xous::MemoryAddress::new(crate::board::CAM_IFRAM_ADDR),
                None,
                crate::board::CAM_IFRAM_LEN_PAGES * 4096,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )?;
            let cam_ifram = unsafe {
                crate::ifram::IframRange::from_raw_parts(
                    crate::board::CAM_IFRAM_ADDR,
                    ifram.as_ptr() as usize,
                    ifram.len(),
                )
            };
            self.ifram = Some(cam_ifram);
        }
        Ok(())
    }

    pub fn has_ifram(&self) -> bool { self.ifram.is_some() }

    pub fn poke(&self, i2c: &mut dyn I2cApi, adr: u8, dat: u8) {
        i2c.i2c_write(OV2640_DEV, adr, &[dat]).expect("write failed");
    }

    // chip does not support sequential reads
    pub fn peek(&self, i2c: &mut dyn I2cApi, adr: u8, dat: &mut [u8]) {
        for (i, d) in dat.iter_mut().enumerate() {
            let mut one_byte = [0u8];
            i2c.i2c_read(OV2640_DEV, adr + i as u8, &mut one_byte, false).expect("read failed");
            *d = one_byte[0];
        }
    }

    pub fn init(&mut self, i2c: &mut dyn I2cApi, resolution: Resolution) {
        self.poke(i2c, OV2640_DSP_RA_DLMT, 0x1);
        self.poke(i2c, OV2640_SENSOR_COM7, 0x80);
        match resolution {
            Resolution::Res480x272 => {
                for poke_pair in super::tables::OV2640_480X272.iter() {
                    self.poke(i2c, poke_pair[0], poke_pair[1]);
                }
            }
            Resolution::Res640x480 => {
                for poke_pair in super::tables::OV2640_VGA.iter() {
                    self.poke(i2c, poke_pair[0], poke_pair[1]);
                }
            }
            Resolution::Res320x240 => {
                for poke_pair in super::tables::OV2640_QVGA.iter() {
                    self.poke(i2c, poke_pair[0], poke_pair[1]);
                }
            }
            Resolution::Res160x120 => {
                for poke_pair in super::tables::OV2640_QQVGA.iter() {
                    self.poke(i2c, poke_pair[0], poke_pair[1]);
                }
            }
            Resolution::Res256x256 => {
                // this does not work
                for poke_pair in super::tables::OV2640_VGA.iter() {
                    self.poke(i2c, poke_pair[0], poke_pair[1]);
                }
                // mode is in 0xFF == 0
                self.poke(i2c, 0x5a, 0x40);
                self.poke(i2c, 0x5b, 0x40);
            }
        }
        self.resolution = resolution;

        // Setup YUV output mode
        self.poke(&mut i2c, 0xFF, 0x00);
        self.poke(&mut i2c, 0xDA, 0x01); // YUV LE

        // set sync polarity
        let vsync_pol = 0;
        let hsync_pol = match resolution {
            Resolution::Res320x240 => 0,
            _ => 0,
        };
        self.csr.wo(
            utra::udma_camera::REG_CAM_VSYNC_POLARITY,
            self.csr.ms(utra::udma_camera::REG_CAM_VSYNC_POLARITY_R_CAM_VSYNC_POLARITY, vsync_pol)
                | self.csr.ms(utra::udma_camera::REG_CAM_VSYNC_POLARITY_R_CAM_HSYNC_POLARITY, hsync_pol),
        );

        // multiply by 1
        self.csr.wo(utra::udma_camera::REG_CAM_CFG_FILTER, 0x01_01_01);

        let (x, _y) = resolution.into();
        self.csr.wo(utra::udma_camera::REG_CAM_CFG_SIZE, (x as u32 - 1) << 16);

        let global = self.csr.ms(CFG_FRAMEDROP_EN, 0)
            | self.csr.ms(CFG_FORMAT, Format::BypassLe as u32)
            | self.csr.ms(CFG_FRAMESLICE_EN, 0)
            | self.csr.ms(CFG_SHIFT, 0);
        self.csr.wo(utra::udma_camera::REG_CAM_CFG_GLOB, global);
    }

    /// TODO: figure out how to length-bound this to...the frame slice size? line size? idk...
    pub fn rx_buf<T: UdmaWidths>(&self) -> &[T] { &self.ifram.as_ref().unwrap().as_slice() }

    /// TODO: figure out how to length-bound this to...the frame slice size? line size? idk...
    pub unsafe fn rx_buf_phys<T: UdmaWidths>(&self) -> &[T] { &self.ifram.as_ref().unwrap().as_phys_slice() }

    /// TODO: Rework this to use the frame sync + automatic re-initiation on capture_await() for frames
    /// TODO: Also make an interrupt driven version of this.
    pub fn capture_async(&mut self) {
        // we want the sliced resolution so resolve resolution through the method call wrapper
        let (cols, rows) = self.resolution();
        let total_len = rows * cols;
        self.csr.rmwf(CFG_GLOB_EN, 1);
        unsafe { self.udma_enqueue(Bank::Rx, &self.rx_buf_phys::<u16>()[..total_len], CFG_EN | CFG_SIZE_16) }
    }

    pub fn capture_await(&mut self, _use_yield: bool) {
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            if _use_yield {
                xous::yield_slice();
            }
        }
    }

    pub fn resolution(&self) -> (usize, usize) {
        if let Some((x, y)) = self.slicing { (x, y) } else { self.resolution.into() }
    }

    pub fn set_slicing(&mut self, ll: (usize, usize), ur: (usize, usize)) {
        let (llx, lly) = ll;
        let (urxx, uryy) = ur;
        let urx = urxx.saturating_sub(1);
        let ury = uryy.saturating_sub(1);
        self.csr.wo(utra::udma_camera::REG_CAM_CFG_LL, llx as u32 & 0xFFFF | ((lly as u32 & 0xFFFF) << 16));
        self.csr.wo(utra::udma_camera::REG_CAM_CFG_UR, urx as u32 & 0xFFFF | ((ury as u32 & 0xFFFF) << 16));
        self.csr.rmwf(CFG_FRAMESLICE_EN, 1);
        self.slicing = Some((urxx - llx, uryy - lly));
        // self.csr.wo(utra::udma_camera::REG_CAM_CFG_SIZE, (urx - llx) as u32 - 1);
    }

    pub fn disable_slicing(&mut self) {
        self.csr.rmwf(CFG_FRAMESLICE_EN, 0);
        self.slicing = None;
    }

    pub fn color_mode(&mut self, i2c: &mut dyn I2cApi, mode: ColorMode) {
        self.poke(i2c, 0xff, 0x00);
        self.poke(i2c, 0x7c, 0x00);
        self.poke(i2c, 0x7d, mode as u8);
        self.poke(i2c, 0x7c, 0x05);
        self.poke(i2c, 0x7d, 0x80);
        self.poke(i2c, 0x7d, 0x80);
        self.color_mode = mode;
    }

    pub fn contrast_brightness(&mut self, i2c: &mut dyn I2cApi, contrast: Contrast, brightness: Brightness) {
        self.contrast = contrast;
        self.brightness = brightness;
        let c = (contrast as u16).to_le_bytes();
        self.poke(i2c, 0xff, 0x00);
        self.poke(i2c, 0x7c, 0x00);
        self.poke(i2c, 0x7d, 0x04);
        self.poke(i2c, 0x7c, 0x07);
        self.poke(i2c, 0x7d, brightness as u8);
        self.poke(i2c, 0x7d, c[0]);
        self.poke(i2c, 0x7d, c[1]);
        self.poke(i2c, 0x7d, 0x06);
    }

    pub fn effect(&mut self, i2c: &mut dyn I2cApi, effect: Effect) {
        let e = (effect as u16).to_le_bytes();
        self.poke(i2c, 0xff, 0x00);
        self.poke(i2c, 0x7c, 0x00);
        self.poke(i2c, 0x7d, 0x18);
        self.poke(i2c, 0x7c, 0x05);
        self.poke(i2c, 0x7d, e[0]);
        self.poke(i2c, 0x7d, e[1]);
    }

    /// Returns (product ID, manufacturer ID)
    pub fn read_id(&self, i2c: &mut dyn I2cApi) -> (u16, u16) {
        let mut pid = [0u8; 2];
        let mut mid = [0u8; 2];
        self.poke(i2c, 0xff, 0x01); // select the correct bank
        self.delay(1);
        self.peek(i2c, OV2640_SENSOR_PIDH, &mut pid);
        self.peek(i2c, OV2640_SENSOR_MIDH, &mut mid);
        (u16::from_be_bytes(pid), u16::from_be_bytes(mid))
    }

    pub fn delay(&self, quantum: usize) {
        #[cfg(feature = "std")]
        {
            let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
            tt.sleep_ms(quantum).ok();
        }
        #[cfg(not(feature = "std"))]
        {
            use utralib::{CSR, utra};
            // abuse the d11ctime timer to create some time-out like thing
            let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
            d11c.wfo(utra::d11ctime::CONTROL_COUNT, 333_333); // 1.0ms per interval
            let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
            for _ in 0..quantum {
                while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
                polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
            }
            // we have to split this because we don't know where we caught the previous interval
            if quantum == 1 {
                while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
            }
        }
    }
}
