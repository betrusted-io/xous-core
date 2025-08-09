use cramium_api::camera::*;
use cramium_api::*;
use utralib::CSR;
use utralib::utra;
use utralib::utra::udma_camera::REG_CAM_CFG_GLOB;

use super::constants::*;
use super::tables::*;
use crate::ifram::IframRange;
use crate::udma::Udma;
use crate::udma::*;

pub const GC2145_DEV: u8 = 0x3C;

pub const CFG_FRAMEDROP_EN: utralib::Field = utralib::Field::new(1, 0, REG_CAM_CFG_GLOB);
pub const CFG_FRAMEDROP_VAL: utralib::Field = utralib::Field::new(6, 1, REG_CAM_CFG_GLOB);
pub const CFG_FRAMESLICE_EN: utralib::Field = utralib::Field::new(1, 7, REG_CAM_CFG_GLOB);
pub const CFG_FORMAT: utralib::Field = utralib::Field::new(3, 8, REG_CAM_CFG_GLOB);
pub const CFG_SHIFT: utralib::Field = utralib::Field::new(4, 11, REG_CAM_CFG_GLOB);
pub const CFG_SOF_SYNC: utralib::Field = utralib::Field::new(1, 30, REG_CAM_CFG_GLOB);
pub const CFG_GLOB_EN: utralib::Field = utralib::Field::new(1, 31, REG_CAM_CFG_GLOB);

pub struct Gc2145 {
    csr: CSR<u32>,
    ifram: Option<IframRange>,
    resolution: Resolution,
    slicing: Option<(usize, usize)>,
}

impl Udma for Gc2145 {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}

impl Gc2145 {
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
        Ok(Gc2145::new_with_ifram(ifram))
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
        i2c.i2c_write(GC2145_DEV, adr, &[dat]).expect("write failed");
    }

    // chip does not support sequential reads
    pub fn peek(&self, i2c: &mut dyn I2cApi, adr: u8, dat: &mut [u8]) {
        for (i, d) in dat.iter_mut().enumerate() {
            let mut one_byte = [0u8];
            i2c.i2c_read(GC2145_DEV, adr + i as u8, &mut one_byte, false).expect("read failed");
            *d = one_byte[0];
        }
    }

    fn gc2145_set_window(&self, i2c: &mut dyn I2cApi, mut reg: u8, x: u16, y: u16, w: u16, h: u16) {
        self.poke(i2c, GC2145_REG_RESET, GC2145_SET_P0_REGS);

        /* Y/row offset */
        self.poke(i2c, reg, (y >> 8) as u8);
        reg += 1;
        self.poke(i2c, reg, (y & 0xff) as u8);
        reg += 1;

        /* X/col offset */
        self.poke(i2c, reg, (x >> 8) as u8);
        reg += 1;
        self.poke(i2c, reg, (x & 0xff) as u8);
        reg += 1;

        /* Window height */
        self.poke(i2c, reg, (h >> 8) as u8);
        reg += 1;
        self.poke(i2c, reg, (h & 0xff) as u8);
        reg += 1;

        /* Window width */
        self.poke(i2c, reg, (w >> 8) as u8);
        reg += 1;
        self.poke(i2c, reg, (w & 0xff) as u8);
    }

    fn set_resolution(&self, i2c: &mut dyn I2cApi, w: u16, h: u16) {
        // TODO: figure out what this parameter even means in the context of an...unconstrained variable
        // resolution? Maybe? It's not even clear to me that was the original intent from the C driver.

        // 160x120 base scaling
        // let c_ratio = 4u16;
        // let r_ratio = 4u16;
        // 320x240 base scaling
        let c_ratio = 3u16;
        let r_ratio = 3u16;
        // 640x480 base scaling
        // let c_ratio = 2u16;
        // let r_ratio = 2u16;

        /* Calculates the window boundaries to obtain the desired resolution */
        let win_w = w * c_ratio;
        let win_h = h * r_ratio;
        let x = ((win_w / c_ratio) - w) / 2;
        let y = ((win_h / r_ratio) - h) / 2;
        let win_x = (UXGA_HSIZE - win_w) / 2;
        let win_y = (UXGA_VSIZE - win_h) / 2;

        /* Set readout window first. */
        self.gc2145_set_window(i2c, GC2145_REG_BLANK_WINDOW_BASE, win_x, win_y, win_w + 16, win_h + 8);

        /* Set cropping window next. */
        self.gc2145_set_window(i2c, GC2145_REG_WINDOW_BASE, x, y, w, h);

        /* Enable crop */
        self.poke(i2c, GC2145_REG_CROP_ENABLE, GC2145_CROP_SET_ENABLE);
        // self.poke(i2c, GC2145_REG_CROP_ENABLE, 0);

        /* Set Sub-sampling ratio and mode */
        self.poke(i2c, GC2145_REG_SUBSAMPLE, ((r_ratio << 4) | c_ratio) as u8);

        self.poke(i2c, GC2145_REG_SUBSAMPLE_MODE, GC2145_SUBSAMPLE_MODE_SMOOTH);

        self.delay(30);

        // divide the clock down, so that the system can keep up.
        // self.poke(i2c, 0xFA, 0x32); // this offers a higher frame rate, but a greater bus congestion -
        // maybe available on NTO
        // self.poke(i2c, 0xFA, 0x63); // this is necessary for MPW due to SPI backpressure bug
        self.poke(i2c, 0xFA, 0x52); // this is necessary for MPW due to SPI backpressure bug
    }

    #[inline(never)]
    pub fn init(&mut self, i2c: &mut dyn I2cApi, resolution: Resolution) {
        // initiate a reset
        self.poke(i2c, GC2145_REG_RESET, GC2145_REG_SW_RESET);
        self.delay(300); // wait for reset

        // do the init pokes, these settings are from the zephyr-OS reference code
        for &[adr, dat] in GC2145_INIT.iter() {
            self.poke(i2c, adr, dat);
        }
        // setup AEC, these settings are yanked out of the Linux kernel
        for &[adr, dat] in GC2145_AEC.iter() {
            self.poke(i2c, adr, dat);
        }

        // set up YUV mode
        self.poke(i2c, GC2145_REG_RESET, GC2145_SET_P0_REGS);
        self.delay(30);
        let mut buf = [0u8; 1];
        self.peek(i2c, GC2145_REG_OUTPUT_FMT, &mut buf);
        self.delay(30);
        self.poke(
            i2c,
            GC2145_REG_OUTPUT_FMT,
            (buf[0] & !GC2145_REG_OUTPUT_FMT_MASK) | GC2145_REG_OUTPUT_FMT_YCBYCR,
        );
        self.delay(30);

        let (w, h) = resolution.into();
        crate::println!("resolution set to {}x{}", w, h);
        self.set_resolution(i2c, w as u16, h as u16);
        self.resolution = resolution;

        crate::println!("udma setup");
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
            | self.csr.ms(CFG_SOF_SYNC, 1)
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

    /// Returns (product ID, manufacturer ID)
    /// Should be 0x2155, 0x0078
    pub fn read_id(&self, i2c: &mut dyn I2cApi) -> (u16, u16) {
        let mut pid = [0u8; 2];
        let mut did = [0u8; 1];
        self.peek(i2c, GC2145_PIDH, &mut pid); // should return 0x2155
        self.peek(i2c, GC2145_I2C_ID, &mut did);
        (u16::from_be_bytes(pid), did[0] as u16)
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
