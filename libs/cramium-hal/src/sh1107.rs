// `Command` vendored from https://github.com/ithinuel/sh1107-rs/tree/main
use crate::{
    ifram::IframRange,
    iox::{IoGpio, IoSetup, IoxPort},
    udma::{PeriphId, Spim, SpimClkPha, SpimClkPol, SpimCs, UdmaGlobalConfig},
};

pub const COLUMN: u8 = 128;
pub const ROW: u8 = 128;
pub const PAGE: u8 = ROW / 8;

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

#[repr(usize)]
#[derive(Eq, PartialEq, Copy, Clone)]
enum BufferState {
    A = 0,
    B = 2048,
}
impl BufferState {
    pub fn swap(self) -> Self { if self == BufferState::A { BufferState::B } else { BufferState::A } }

    pub fn as_index(&self) -> usize {
        match self {
            BufferState::A => 0,
            BufferState::B => 1,
        }
    }
}

pub struct Oled128x128<'a> {
    spim: Spim,
    // front and back buffers
    buffers: [&'static mut [u8]; 2],
    // length of the sideband memory region for queuing commands to the OLED. Must be allocated
    // immediately after the total frame buffer length
    pub sideband_len: usize,
    active_buffer: BufferState,
    cd_port: IoxPort,
    cd_pin: u8,
    iox: &'a dyn IoGpio,
}

impl<'a> Oled128x128<'a> {
    pub fn new<T>(perclk_freq: u32, iox: &'a T, udma_global: &'a dyn UdmaGlobalConfig) -> Self
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
                // 2x buffers reserved: (128 * 128 / 8) * 2 = 4096
                // Add 256 for display commands.
                // Internally, an extra 16 is added for UDMA SPIM commands (these are commands
                // to the SPIM hardware itself, not transmitted to the display).
                // The +16 is not reported here because it's out of band, but we need to allocate
                // an IFRAM range large enough to accommodate that. However, because we have
                // to round up any allocations to a full page length, we end up with extra
                // unused space.
                4096 + 256, // the 256 is for direct commands
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
        Self {
            spim,
            // safety: this is safe because these ranges are in fact allocated, and all values can be
            // represented. They have a static lifetime because they are mapped to hardware. There is
            // in fact an unsafe front/back buffer contention, but that's the whole point of this routine,
            // to safely manage that.
            buffers: [unsafe { core::slice::from_raw_parts_mut(ifram_vaddr as *mut u8, 2048) }, unsafe {
                core::slice::from_raw_parts_mut((ifram_vaddr + 2048) as *mut u8, 2048)
            }],
            sideband_len: 256,
            active_buffer: BufferState::A,
            cd_port,
            cd_pin,
            iox,
        }
    }

    fn set_data(&self) { self.iox.set_gpio_pin_value(self.cd_port, self.cd_pin, crate::iox::IoxValue::High); }

    fn set_command(&self) {
        self.iox.set_gpio_pin_value(self.cd_port, self.cd_pin, crate::iox::IoxValue::Low);
    }

    pub fn buffer_swap(&mut self) { self.active_buffer = self.active_buffer.swap(); }

    pub fn buffer_mut(&mut self) -> &mut [u8] { self.buffers[self.active_buffer.as_index()] }

    pub fn buffer(&self) -> &[u8] { self.buffers[self.active_buffer.as_index()] }

    /// Transfers the back buffer
    pub fn draw(&mut self) {
        // this must be opposite of what `buffer` / `buffer_mut` returns
        let buffer_start = self.active_buffer.swap() as usize;
        let chunk_size = 128;
        let chunks = self.buffer().len() / chunk_size;
        // we don't do this with an iterator because it involves an immutable borrow of
        // `buffer`, which prevents us from doing anything with the interface inside the loop.
        for page in 0..chunks {
            // The cs_active() waits are necessary because the UDMA block will eagerly report
            // the transaction is done before the data is done transmitting, and we have to
            // toggle set_data() only after the physical transaction is done, not after the
            // the last UDMA action has been queued.
            self.send_command(Command::SetPageAddress(page as u8).encode());
            self.send_command(Command::SetColumnAddress(0).encode());
            // wait for commands to finish before toggling set_data
            // self.spim.tx_data_await(false);
            // crate::println!("Send page {}, offset {:x}", page, buffer_start + page * chunk_size);
            self.set_data();
            // safety: data is already copied into the DMA buffer. size & len are in bounds.
            unsafe {
                self.spim
                    .txrx_data_async_from_parts::<u8>(
                        buffer_start + page * chunk_size,
                        chunk_size,
                        true,
                        false,
                    )
                    .expect("Couldn't initiate oled data transfer");
            }
            self.spim.txrx_await(false).unwrap();
        }
    }

    pub fn send_command<'b, U>(&'b mut self, cmd: U)
    where
        U: IntoIterator<Item = u8> + 'b,
    {
        self.set_command();
        let total_buf_len = self.buffers.iter().map(|x| x.len()).sum();
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
        self.spim.txrx_await(false).unwrap();
    }

    pub fn init(&mut self) {
        use Command::*;
        let init_sequence = [
            DisplayOnOff(DisplayState::Off),
            SetDCDCSettings(0x0),
            SetStartLine(0),
            SetDisplayOffset(0),
            SetContrastControl(0x4f),
            SetAddressMode(AddressMode::Page),
            SetSegmentReMap(true),
            SetCOMScanDirection(Direction::Normal),
            SetMultiplexRatio(128),
            SetClkDividerOscFrequency { divider: 1, osc_freq_ratio: 5 },
            SetChargePeriods { precharge: Some(2), discharge: 2 },
            SetVCOMHDeselectLevel(0x35),
            SetPageAddress(0),
            ForceEntireDisplay(false),
            SetDisplayMode(DisplayMode::BlackOnWhite),
            DisplayOnOff(DisplayState::On),
        ];

        for command in init_sequence {
            let bytes = command.encode();
            self.send_command(bytes);
        }
    }

    pub fn clear(&mut self) { self.buffer_mut().fill(0); }

    pub fn put_pixel(&mut self, x: u8, y: u8, on: bool) {
        if x > COLUMN || y > ROW {
            return;
        }
        let buffer = self.buffer_mut();
        if on {
            buffer[x as usize + (y as usize / 8) * COLUMN as usize] |= 1 << (y % 8);
        } else {
            buffer[x as usize + (y as usize / 8) * COLUMN as usize] &= !(1 << (y % 8));
        }
    }
}
