// `Command` vendored from https://github.com/ithinuel/sh1107-rs/tree/main
use bao1x_api::EventChannel;
use bao1x_api::*;
use ux_api::minigfx::{ColorNative, FrameBuffer, Point};
use ux_api::platform::*;

use crate::ifram::IframRange;
use crate::udma::{self, CommandSet, Spim, SpimClkPha, SpimClkPol, SpimCs};

pub const COLUMN: isize = WIDTH;
pub const ROW: isize = LINES;
pub const PAGE: u8 = ROW as u8 / 8;

// IFRAM space reserved for UDMA channel commands
const SIDEBAND_LEN: usize = 256;

// 0x4f is a bit too bright
pub const DEFAULT_BRIGHTNESS: u8 = 0x3f;

pub struct MainThreadToken(());
impl MainThreadToken {
    pub fn new() -> Self { MainThreadToken(()) }
}
pub enum Never {}

/// Shim for hosted mode compatibility
#[inline]
pub fn claim_main_thread(f: impl FnOnce(MainThreadToken) -> Never + Send + 'static) -> ! {
    // Just call the closure - this backend will work on any thread
    #[allow(unreachable_code)] // false positive
    match f(MainThreadToken(())) {}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonoColor(ColorNative);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mono {
    Black,
    White,
}
impl From<ColorNative> for Mono {
    fn from(value: ColorNative) -> Self {
        match value.0 {
            1 => Mono::Black,
            _ => Mono::White,
        }
    }
}
impl Into<ColorNative> for Mono {
    fn into(self) -> ColorNative {
        match self {
            Mono::Black => ColorNative::from(1),
            Mono::White => ColorNative::from(0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayState {
    Off,
    On,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Normal,
    Inverted,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddressMode {
    Page,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisplayMode {
    BlackOnWhite,
    WhiteOnBlack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    SetColumnAddress(u8),
    SetAddressMode(AddressMode),
    SetDisplayMode(DisplayMode),
    ForceEntireDisplay(bool),
    SetClkDividerOscFrequency {
        divider: u8,
        osc_freq_ratio: i8,
    },
    SetMultiplexRatio(u8),
    SetStartLine(u8),
    SetSegmentReMap(bool),
    SetCOMScanDirection(Direction),
    SetDisplayOffset(u8),
    SetContrastControl(u8),
    /// Set Charge & Discharge period
    SetChargePeriods {
        precharge: Option<u8>,
        discharge: u8,
    },
    SetVCOMHDeselectLevel(u8),
    SetDCDCSettings(u8),
    DisplayOnOff(DisplayState),
    SetPageAddress(u8),
    StartReadModifyWrite,
    EndReadModifyWrite,
    Nop,
}

impl Command {
    fn encode(self) -> impl Iterator<Item = u8> {
        use either::Either::*;
        match self {
            Self::SetColumnAddress(addr) => {
                assert!(addr < 128);
                Right([addr & 0xF, 0x10 | ((addr & 0x70) >> 4)])
            }
            Self::SetAddressMode(mode) => Left(0x20 | if let AddressMode::Page = mode { 0 } else { 1 }),
            Self::SetContrastControl(contrast) => Right([0x81, contrast]),
            Self::SetSegmentReMap(is_remapped) => Left(0xA0 | if is_remapped { 1 } else { 0 }),
            Self::SetMultiplexRatio(ratio) => {
                assert!((1..=128).contains(&ratio));
                Right([0xA8, ratio - 1])
            }
            Self::ForceEntireDisplay(state) => Left(0xA4 | if state { 1 } else { 0 }),
            Self::SetDisplayMode(mode) => {
                Left(0xA6 | if let DisplayMode::WhiteOnBlack = mode { 1 } else { 0 })
            }
            Self::SetDisplayOffset(offset) => Right([0xD3, offset & 0x7F]),
            Self::SetDCDCSettings(cfg) => Right([0xAD, 0x80 | (cfg & 0x0F)]),
            Self::DisplayOnOff(state) => Left(0xAE | if let DisplayState::On = state { 1 } else { 0 }),
            Self::SetPageAddress(addr) => {
                assert!(addr < 16);
                Left(0xB0 | (addr & 0x0F))
            }
            Self::SetCOMScanDirection(dir) => Left(0xC0 | if let Direction::Normal = dir { 0 } else { 0x08 }),
            Self::SetClkDividerOscFrequency { divider, osc_freq_ratio } => {
                assert!(osc_freq_ratio % 5 == 0, "osc_freq_ratio must be a multiple of 5.");
                assert!((-25..=50).contains(&osc_freq_ratio), "osc_freq_ratio must be within [-25; 50]");
                assert!((1..=16).contains(&divider), "divider must be in [1; 16]");

                let osc_freq_ratio = osc_freq_ratio / 5 + 5;
                Right([0xD5, ((osc_freq_ratio & 0xF) << 4) as u8 | (divider - 1)])
            }
            Self::SetChargePeriods { precharge, discharge } => {
                let precharge = if let Some(v) = precharge {
                    assert!((1..=15).contains(&v));
                    v
                } else {
                    0
                };
                assert!((1..=15).contains(&discharge));
                let arg = discharge << 4 | precharge;

                Right([0xD9, arg])
            }
            Self::SetVCOMHDeselectLevel(arg) => Right([0xDB, arg]),
            Self::SetStartLine(line) => {
                assert!(line < 128);

                Right([0xDC, line & 0x7F])
            }
            Self::StartReadModifyWrite => Left(0xE0),
            Self::EndReadModifyWrite => Left(0xEE),
            Self::Nop => Left(0xE3),
        }
        .map_left(|v| [v])
        .into_iter()
    }
}

pub struct Oled128x128<'a> {
    spim: Spim,
    hw_buf: &'static mut [u32],
    buffer: [u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
    stash: [u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
    // length of the sideband memory region for queuing commands to the OLED. Must be allocated
    // immediately after the total frame buffer length
    pub sideband_len: usize,
    cd_port: IoxPort,
    cd_pin: u8,
    iox: &'a dyn IoGpio,
    powerdown: bool,
    power_port: IoxPort,
    power_pin: u8,
    clk_div: u8,
}

impl<'a> Oled128x128<'a> {
    pub fn new<T>(
        _main_thread_token: MainThreadToken,
        perclk_freq: u32,
        iox: &'a T,
        udma_global: &'a dyn UdmaGlobalConfig,
    ) -> Self
    where
        T: IoSetup + IoGpio,
    {
        let (channel, cd_port, cd_pin, _cs_pin) = crate::board::setup_display_pins(iox);
        udma_global.clock(PeriphId::from(channel), true);
        #[cfg(not(feature = "std"))]
        let ifram_vaddr = crate::board::DISPLAY_IFRAM_ADDR;
        #[cfg(feature = "std")]
        let ifram_vaddr = {
            xous::map_memory(
                xous::MemoryAddress::new(crate::board::DISPLAY_IFRAM_ADDR),
                None,
                4096 * 2,
                xous::MemoryFlags::R | xous::MemoryFlags::W,
            )
            .expect("couldn't map window for display IFRAM")
            .as_ptr() as usize
        };
        // safety: safe because the udma clock is turned on, and IFRAM is pulling from
        // statically allocated bank
        let mut spim = unsafe {
            Spim::new_with_ifram(
                channel,
                2_000_000,
                perclk_freq / 2,
                SpimClkPol::LeadingEdgeRise,
                SpimClkPha::CaptureOnLeading,
                SpimCs::Cs0,
                0,
                0,
                None,
                // 1x buffers reserved: (128 * 128 / 8) * 1 = 2048
                // Add 2048 for the dummy Rx (to ensure that the command actually goes through)
                // Add 256 for display commands.
                // Internally, an extra 16 is added for UDMA SPIM commands (these are commands
                // to the SPIM hardware itself, not transmitted to the display).
                // The +16 is not reported here because it's out of band, but we need to allocate
                // an IFRAM range large enough to accommodate that. However, because we have
                // to round up any allocations to a full page length, we end up with extra
                // unused space.
                2048 + 256, // the 256 is for direct commands
                // the 2048 is for dummy-Rx so we can properly measure when the transaction is done
                2048,
                None,
                None,
                // Note: the IFRAM needs to be 16 bytes longer than the data range to accommodate
                // command sending. But because we have to round up to a whole page, we end up wasting 4080
                // bytes.
                IframRange::from_raw_parts(crate::board::DISPLAY_IFRAM_ADDR, ifram_vaddr, 4096 * 2),
            )
        };
        spim.set_endianness(crate::udma::SpimEndian::MsbFirst);
        let (power_port, power_pin) = crate::board::setup_oled_power_pin(iox);
        crate::board::setup_periph_reset_pin(iox);
        Self {
            spim,
            // safety: this is safe because these ranges are in fact allocated, and all values can be
            // represented. They have a static lifetime because they are mapped to hardware. There is
            // in fact an unsafe front/back buffer contention, but that's the whole point of this routine,
            // to safely manage that.
            hw_buf: unsafe {
                core::slice::from_raw_parts_mut(
                    ifram_vaddr as *mut u32,
                    WIDTH as usize * HEIGHT as usize / (size_of::<u32>() * 8),
                )
            },
            buffer: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
            stash: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
            sideband_len: SIDEBAND_LEN,
            cd_port,
            cd_pin,
            iox,
            power_port,
            power_pin,
            powerdown: false,
            clk_div: ((perclk_freq / 2) / 2_000_000) as u8,
        }
    }

    // used to recover a wedged SPI interface
    pub fn reinit_spi(&mut self) {
        self.spim.send_cmd_list(&[crate::udma::SpimCmd::Config(
            SpimClkPol::LeadingEdgeRise,
            SpimClkPha::CaptureOnLeading,
            self.clk_div,
        )]);
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
    pub unsafe fn to_raw_parts(
        &self,
    ) -> (
        (
            usize,
            udma::SpimCs,
            u8,
            u8,
            Option<EventChannel>,
            udma::SpimMode,
            udma::SpimByteAlign,
            IframRange,
            usize,
            usize,
            u8,
            Option<CommandSet>,
        ),
        bool,
        u8,
    ) {
        (self.spim.into_raw_parts(), self.powerdown, self.clk_div)
    }

    /// Creates a clone of the display handle. This is only safe if the handles are used in a
    /// mutually excluslive fashion, and the handles are shared only within the same process space
    /// (i.e. between threads in a single process). The primary purpose for this to exist is to create
    /// a dedicated display object inside a panic handler thread.
    ///
    /// The outer implementation of the main loop with the panic handler has to enforce the mutual exclusion
    /// property, otherwise unpredictable behavior may occur.
    ///
    /// A fresh reference to the iox object is required, so that the lifetimes of the iox object are not
    /// entangled between the original reference and the clone.
    pub unsafe fn from_raw_parts<T>(
        display_parts: (
            (
                usize,
                udma::SpimCs,
                u8,
                u8,
                Option<EventChannel>,
                udma::SpimMode,
                udma::SpimByteAlign,
                IframRange,
                usize,
                usize,
                u8,
                Option<CommandSet>,
            ),
            bool,
            u8,
        ),
        iox: &'a T,
    ) -> Self
    where
        T: IoGpio + IoSetup,
    {
        // extract the raw parts
        let (
            (
                csr,
                cs,
                sot_wait,
                eot_wait,
                event_channel,
                mode,
                _align,
                ifram,
                tx_buf_len_bytes,
                rx_buf_len_bytes,
                dummy_cycles,
                command_set,
            ),
            powerdown,
            clk_div,
        ) = display_parts;
        // compile them into a new object
        let mut spim = unsafe {
            Spim::from_raw_parts(
                csr,
                cs,
                sot_wait,
                eot_wait,
                event_channel,
                mode,
                _align,
                ifram,
                tx_buf_len_bytes,
                rx_buf_len_bytes,
                dummy_cycles,
                command_set,
            )
        };
        spim.set_endianness(crate::udma::SpimEndian::MsbFirst);
        let ifram_vaddr = spim.ifram.virt_range.as_mut_ptr();
        let (_channel, cd_port, cd_pin, _cs_pin) = crate::board::get_display_pins();
        let (power_port, power_pin) = crate::board::setup_oled_power_pin(iox);
        crate::board::setup_periph_reset_pin(iox);
        Self {
            spim,
            // safety: this is safe because these ranges are in fact allocated, and all values can be
            // represented. They have a static lifetime because they are mapped to hardware. There is
            // in fact an unsafe front/back buffer contention, but that's the whole point of this routine,
            // to safely manage that.
            hw_buf: unsafe {
                core::slice::from_raw_parts_mut(
                    ifram_vaddr as *mut u32,
                    WIDTH as usize * HEIGHT as usize / (size_of::<u32>() * 8),
                )
            },
            buffer: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
            stash: [0u32; WIDTH as usize * HEIGHT as usize / (core::mem::size_of::<u32>() * 8)],
            sideband_len: SIDEBAND_LEN,
            cd_port,
            cd_pin,
            iox,
            powerdown,
            power_pin,
            power_port,
            clk_div,
        }
    }

    pub fn buffer_mut(&mut self) -> &mut ux_api::platform::FbRaw { &mut self.buffer }

    pub fn buffer(&self) -> &ux_api::platform::FbRaw { &self.buffer }

    pub fn screen_size(&self) -> Point { Point::new(WIDTH, LINES) }

    pub fn redraw(&mut self) -> Result<(), xous::Error> { self.draw() }

    pub fn blit_screen(&mut self, bmp: &[u32]) { self.buffer.copy_from_slice(bmp); }

    pub fn set_devboot(&mut self, _ena: bool) {
        unimplemented!("devboot feature does not exist on this platform");
    }

    pub fn stash(&mut self) { self.stash.copy_from_slice(&self.buffer); }

    pub fn pop(&mut self) -> Result<(), xous::Error> {
        self.buffer.copy_from_slice(&self.stash);
        self.redraw()
    }

    fn set_data(&self) { self.iox.set_gpio_pin_value(self.cd_port, self.cd_pin, IoxValue::High); }

    fn set_command(&self) { self.iox.set_gpio_pin_value(self.cd_port, self.cd_pin, IoxValue::Low); }

    pub fn send_command<'b, U>(&'b mut self, cmd: U) -> Result<(), xous::Error>
    where
        U: IntoIterator<Item = u8> + 'b,
    {
        if self.powerdown {
            return Ok(());
        }
        self.set_command();
        let total_buf_len = self.buffer.len() * size_of::<u32>();
        let mut len = 0; // track the full length of the iterator
        // emplace the command in the sideband area, which is after both frame buffers
        // crate::print!("cmd: ");
        let ifram_raw: &mut [u8] = self.spim.tx_buf_mut();
        for (src, dst) in cmd.into_iter().zip(ifram_raw[total_buf_len..].iter_mut()) {
            // crate::print!("{:x} ", src);
            *dst = src;
            len += 1;
        }
        // crate::println!("");
        // safety: data is already copied into the DMA buffer. size & len are in bounds.
        unsafe {
            self.spim
                .txrx_data_async_from_parts::<u8>(total_buf_len, len, true, false)
                .expect("Couldn't initiate oled command");
        }
        self.spim
            .txrx_await(false)
            .inspect_err(|_| {
                #[cfg(feature = "std")]
                log::error!("timeout in send_command");
            })
            .map(|_| ())
    }

    pub fn init(&mut self) -> Result<(), xous::Error> {
        if self.powerdown {
            return Ok(());
        }
        use Command::*;
        let init_sequence = [
            DisplayOnOff(DisplayState::Off),
            SetDCDCSettings(0x0),
            SetStartLine(0),
            SetDisplayOffset(0),
            SetContrastControl(DEFAULT_BRIGHTNESS),
            SetAddressMode(AddressMode::Column),
            SetSegmentReMap(false),
            SetCOMScanDirection(Direction::Inverted),
            SetMultiplexRatio(128),
            SetClkDividerOscFrequency { divider: 1, osc_freq_ratio: 5 },
            SetChargePeriods { precharge: Some(2), discharge: 2 },
            SetVCOMHDeselectLevel(0x35),
            SetPageAddress(0),
            ForceEntireDisplay(false),
            SetDisplayMode(DisplayMode::WhiteOnBlack),
        ];

        for command in init_sequence {
            let bytes = command.encode();
            self.send_command(bytes)?;
        }
        // clear the frame buffer
        self.buffer_mut().fill(0xFFFF_FFFF);
        self.draw()?;

        let display_on = [DisplayOnOff(DisplayState::On)];
        for command in display_on {
            let bytes = command.encode();
            self.send_command(bytes)?;
        }
        Ok(())
    }

    pub fn flip_vertical(&mut self, flip: bool) -> Result<(), xous::Error> {
        use Command::*;
        let cmds = if flip {
            // flipped
            [SetSegmentReMap(true), SetCOMScanDirection(Direction::Normal)]
        } else {
            // normal
            [SetSegmentReMap(false), SetCOMScanDirection(Direction::Inverted)]
        };
        for c in cmds {
            let bytes = c.encode();
            self.send_command(bytes)?;
        }
        Ok(())
    }

    pub fn powerdown(&mut self) {
        self.powerdown = true;
        let powoff = [Command::DisplayOnOff(DisplayState::Off)];
        for command in powoff {
            let bytes = command.encode();
            self.send_command(bytes).ok();
        }
        // assert reset
        crate::board::assert_periph_reset(self.iox, true);
        self.iox.set_gpio_pin_value(self.cd_port, self.cd_pin, IoxValue::Low);
        // cut power
        self.iox.set_gpio_pin_value(self.power_port, self.power_pin, IoxValue::Low);
    }

    /// Safety: this must be followed-up by a call to init(). The reason it's not bundled into
    /// this call is we need to introduce a platform-dependent delay of a couple milliseconds
    /// before calling init(), and we don't want to put the platform-dependent delay into this driver.
    pub unsafe fn powerup(&mut self) {
        // restore power
        self.iox.set_gpio_pin_value(self.power_port, self.power_pin, IoxValue::High);
        // de-assert reset
        crate::board::assert_periph_reset(self.iox, false);
        self.powerdown = false;
    }

    pub fn brightness(&mut self, level: u8) -> Result<(), xous::Error> {
        let bytes = Command::SetContrastControl(level).encode();
        self.send_command(bytes)
    }

    #[cfg(feature = "std")]
    pub fn render_bitmap(&mut self, bitmap: ux_api::service::api::BaosecBitmap) {
        const W: isize = WIDTH as _;
        const H: isize = LINES as _;

        // Clamp bounding box to valid bitmap dimensions.
        // If br > 128, truncate. If tl < 0, clamp to 0.
        let src_x0 = bitmap.bounding_box.tl.x.max(0);
        let src_y0 = bitmap.bounding_box.tl.y.max(0);
        let src_x1 = bitmap.bounding_box.br.x.min(W);
        let src_y1 = bitmap.bounding_box.br.y.min(H);

        for src_y in src_y0..src_y1 {
            // Map this bitmap row to its destination row in the framebuffer.
            let dst_y = src_y + bitmap.top_left.y;

            // Skip rows that land outside the framebuffer.
            if dst_y < 0 || dst_y >= H {
                continue;
            }

            for src_x in src_x0..src_x1 {
                // Map this bitmap column to its destination column.
                let dst_x = src_x + bitmap.top_left.x;

                // Skip columns that land outside the framebuffer.
                if dst_x < 0 || dst_x >= W {
                    continue;
                }

                // --- read source bit ---
                let src_linear = (src_x + src_y * W) as usize;
                let src_word = src_linear >> 5; // / 32
                let src_shift = src_linear & 0x1F; // % 32
                let on = (bitmap.bits[src_word] >> src_shift) & 1;

                // --- write destination bit ---
                let dst_linear = (dst_x + dst_y * W) as usize;
                let dst_word = dst_linear >> 5;
                let dst_shift = dst_linear & 0x1F;

                if on != 0 {
                    self.buffer[dst_word] |= 1 << dst_shift;
                } else {
                    self.buffer[dst_word] &= !(1 << dst_shift);
                }
            }
        }
    }

    /// A diffusion-transition effect courtesy of Claude Sonnet 4.6. Query fed in `render_bitmap` code
    /// and asked for a variant that does a "diffusive dither" transition. Applied largely without changes;
    /// the main change was adjusting the seed to be passed, in instead of fixed, so the transition can be
    /// different every call.
    #[cfg(feature = "std")]
    pub fn render_bitmap_diffuse(
        &mut self,
        bitmap: &ux_api::service::api::BaosecBitmap,
        frames: usize,
        seed: u64,
    ) -> Result<(), xous::Error> {
        use std::collections::VecDeque;

        const W: isize = WIDTH as isize;
        const H: isize = LINES as isize;

        // -- 1. Build per-pixel metadata for the destination region --------------
        //
        // We work in framebuffer linear-index space (0..WIDTH*LINES).
        // Three bitmasks, each 512 u32 words == 16 384 bits == one bit per pixel.

        let mut in_region = [0u32; 512]; // 1 = this fb pixel is touched by the bitmap
        let mut target_bit = [0u32; 512]; // desired final value for pixels in region
        let mut settled = [0u32; 512]; // 1 = pixel already written this transition

        // Clamp the source bounding box (mirrors render_bitmap).
        let src_x0 = bitmap.bounding_box.tl.x.max(0);
        let src_y0 = bitmap.bounding_box.tl.y.max(0);
        let src_x1 = bitmap.bounding_box.br.x.min(W);
        let src_y1 = bitmap.bounding_box.br.y.min(H);

        let mut total: usize = 0;

        for src_y in src_y0..src_y1 {
            let dst_y = src_y + bitmap.top_left.y;
            if dst_y < 0 || dst_y >= H {
                continue;
            }
            for src_x in src_x0..src_x1 {
                let dst_x = src_x + bitmap.top_left.x;
                if dst_x < 0 || dst_x >= W {
                    continue;
                }

                let src_lin = (src_x + src_y * W) as usize;
                let dst_lin = (dst_x + dst_y * W) as usize;

                in_region[dst_lin >> 5] |= 1 << (dst_lin & 31);
                total += 1;

                let bit = (bitmap.bits[src_lin >> 5] >> (src_lin & 31)) & 1;
                if bit != 0 {
                    target_bit[dst_lin >> 5] |= 1 << (dst_lin & 31);
                }
            }
        }

        if total == 0 || frames == 0 {
            return Ok(());
        }

        // -- 2. Collect region pixel indices for random seeding ------------------

        let mut region_pixels: Vec<usize> = Vec::with_capacity(total);
        for i in 0..(WIDTH as usize * LINES as usize) {
            if (in_region[i >> 5] >> (i & 31)) & 1 != 0 {
                region_pixels.push(i);
            }
        }

        // -- 3. Tiny xorshift-64 RNG (no std::collections dependency needed) -----

        let mut rng: u64 = seed ^ total as u64;
        macro_rules! rand {
            () => {{
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng
            }};
        }

        // -- 4. Diffusion loop ----------------------------------------------------

        let pixels_per_frame = (total / frames).max(1);

        // Scatter a handful of seeds so diffusion starts from multiple origins.
        let mut frontier: VecDeque<usize> = VecDeque::new();
        let n_seeds = (total / 32).clamp(1, 64);
        for _ in 0..n_seeds {
            let dl = region_pixels[(rand!() as usize) % total];
            if (settled[dl >> 5] >> (dl & 31)) & 1 == 0 {
                settled[dl >> 5] |= 1 << (dl & 31);
                frontier.push_back(dl);
            }
        }

        let mut committed: usize = 0;

        while committed < total {
            let mut this_frame: usize = 0;

            while this_frame < pixels_per_frame {
                // If the frontier runs dry, plant a new random seed.
                if frontier.is_empty() {
                    let mut planted = false;
                    for _ in 0..64 {
                        let dl = region_pixels[(rand!() as usize) % total];
                        if (settled[dl >> 5] >> (dl & 31)) & 1 == 0 {
                            settled[dl >> 5] |= 1 << (dl & 31);
                            frontier.push_back(dl);
                            planted = true;
                            break;
                        }
                    }
                    if !planted {
                        break;
                    } // all pixels settled
                }

                // -- commit the next frontier pixel -------------------------------
                let dl = frontier.pop_front().unwrap();

                let on = (target_bit[dl >> 5] >> (dl & 31)) & 1;
                if on != 0 {
                    self.buffer[dl >> 5] |= 1 << (dl & 31);
                } else {
                    self.buffer[dl >> 5] &= !(1 << (dl & 31));
                }
                this_frame += 1;

                // -- expand to unsettled cardinal neighbours in the region ---------
                let dst_x = (dl % WIDTH as usize) as isize;
                let dst_y = (dl / WIDTH as usize) as isize;

                for (nx, ny) in
                    [(dst_x - 1, dst_y), (dst_x + 1, dst_y), (dst_x, dst_y - 1), (dst_x, dst_y + 1)]
                {
                    if nx < 0 || nx >= W || ny < 0 || ny >= H {
                        continue;
                    }
                    let ndl = (nx + ny * W) as usize;
                    if (in_region[ndl >> 5] >> (ndl & 31)) & 1 == 0 {
                        continue;
                    }
                    if (settled[ndl >> 5] >> (ndl & 31)) & 1 != 0 {
                        continue;
                    }
                    settled[ndl >> 5] |= 1 << (ndl & 31);
                    frontier.push_back(ndl);
                }
            }

            committed += this_frame;
            if this_frame > 0 {
                self.redraw()?;
            }
            if this_frame == 0 {
                break;
            } // guard against impossible stuck state
        }
        Ok(())
    }
}

impl<'a> FrameBuffer for Oled128x128<'a> {
    /// Copies the SRAM buffer to IFRAM and then transfers that over SPI
    fn draw(&mut self) -> Result<(), xous::Error> {
        self.hw_buf.copy_from_slice(&self.buffer);
        if self.powerdown {
            return Ok(());
        }
        let chunk_size = 16;
        let chunks = self.buffer().len() * size_of::<u32>() / chunk_size;
        // we don't do this with an iterator because it involves an immutable borrow of
        // `buffer`, which prevents us from doing anything with the interface inside the loop.
        for page in 0..chunks {
            // The cs_active() waits are necessary because the UDMA block will eagerly report
            // the transaction is done before the data is done transmitting, and we have to
            // toggle set_data() only after the physical transaction is done, not after the
            // the last UDMA action has been queued.
            self.send_command(Command::SetPageAddress(0).encode())?;
            self.send_command(Command::SetColumnAddress(page as u8).encode())?;
            // wait for commands to finish before toggling set_data
            // self.spim.tx_data_await(false);
            // crate::println!("Send page {}, offset {:x}", page, page * chunk_size);
            self.set_data();
            // safety: data is already copied into the DMA buffer. size & len are in bounds.
            unsafe {
                self.spim
                    .txrx_data_async_from_parts::<u8>(page * chunk_size, chunk_size, true, false)
                    .expect("Couldn't initiate oled data transfer");
            }
            self.spim.txrx_await(false).inspect_err(|_| {
                #[cfg(feature = "std")]
                log::error!("timeout in draw");
            })?;
        }
        Ok(())
    }

    fn clear(&mut self) { self.buffer_mut().fill(0xFFFF_FFFF); }

    fn put_pixel(&mut self, p: Point, on: ColorNative) {
        if p.x >= COLUMN || p.y >= ROW || p.x < 0 || p.y < 0 {
            return;
        }
        let bitnum = (p.x + p.y * COLUMN) as usize;
        if on.0 != 0 {
            self.buffer[bitnum / 32] |= 1 << (bitnum % 32);
        } else {
            self.buffer[bitnum / 32] &= !(1 << (bitnum % 32));
        }
    }

    #[inline(always)]
    unsafe fn put_pixel_unchecked(&mut self, p: Point, on: ColorNative) {
        let bitnum = (p.x + p.y * COLUMN) as usize;
        let word_idx = bitnum >> 5; // bitnum / 32
        let bit_idx = bitnum & 0x1F; // bitnum % 32
        if on.0 != 0 {
            self.buffer[word_idx] |= 1 << bit_idx;
        } else {
            self.buffer[word_idx] &= !(1 << bit_idx);
        }
    }

    fn dimensions(&self) -> Point { Point::new(COLUMN, ROW) }

    fn get_pixel(&self, p: Point) -> Option<ColorNative> {
        if p.x >= COLUMN || p.y >= ROW || p.x < 0 || p.y < 0 {
            return None;
        }
        let bitnum = (p.x + p.y * COLUMN) as usize;
        if self.buffer[bitnum / 32] & 1 << (bitnum % 32) != 0 { Some(1.into()) } else { Some(0.into()) }
    }

    fn xor_pixel(&mut self, p: Point) {
        if let Some(px) = self.get_pixel(p) {
            let mono_px: Mono = px.into();
            self.put_pixel(
                p,
                match mono_px {
                    Mono::Black => Mono::White,
                    Mono::White => Mono::Black,
                }
                .into(),
            );
        }
    }

    /// In this architecture, it's actually totally safe to do this, but the trait
    /// is marked unsafe because in some other displays it may require some tomfoolery
    /// to get reference types to match up.
    unsafe fn raw_mut(&mut self) -> &mut ux_api::platform::FbRaw { self.buffer_mut() }
}
