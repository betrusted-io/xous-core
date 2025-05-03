use cramium_api::*;
use utralib::*;

use crate::ifram::IframRange;
use crate::udma::*;

pub const FLASH_PAGE_LEN: usize = 256;
pub const FLASH_SECTOR_LEN: usize = 4096;
pub const BLOCK_ERASE_LEN: usize = 65536;

// ----------------------------------- SPIM ------------------------------------

/// The SPIM implementation for UDMA does reg-ception, in that they bury
/// a register set inside a register set. The registers are only accessible by,
/// surprise, DMA. The idea behind this is you can load a bunch of commands into
/// memory and just DMA them to the control interface. Sure, cool idea bro.
///
/// Anyways, the autodoc system is unable to extract the register
/// formats for the SPIM. Instead, we have to create a set of hand-crafted
/// structures to deal with this.

#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimClkPol {
    LeadingEdgeRise = 0,
    LeadingEdgeFall = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimClkPha {
    CaptureOnLeading = 0,
    CaptureOnTrailing = 1,
}
#[repr(u32)]
#[derive(Debug, Copy, Clone)]
pub enum SpimCs {
    Cs0 = 0,
    Cs1 = 1,
    Cs2 = 2,
    Cs3 = 3,
}
#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SpimMode {
    Standard = 0,
    Quad = 1,
}
#[repr(u32)]
#[derive(Debug, Copy, Clone)]
pub enum SpimByteAlign {
    Enable = 0,
    Disable = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimCheckType {
    Allbits = 0,
    OnlyOnes = 1,
    OnlyZeros = 2,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimEventGen {
    Disabled = 0,
    Enabled = 1,
}
#[repr(u32)]
#[derive(Copy, Clone)]
pub enum SpimWordsPerXfer {
    Words1 = 0,
    Words2 = 1,
    Words4 = 2,
}
#[repr(u32)]
#[derive(Debug, Copy, Clone)]
pub enum SpimEndian {
    MsbFirst = 0,
    LsbFirst = 1,
}
#[derive(Copy, Clone)]
pub enum SpimWaitType {
    Event(EventChannel),
    Cycles(u8),
}
#[derive(Copy, Clone)]
pub enum SpimCmd {
    /// pol, pha, clkdiv
    Config(SpimClkPol, SpimClkPha, u8),
    StartXfer(SpimCs),
    /// mode, cmd_size (5 bits), command value, left-aligned
    SendCmd(SpimMode, u8, u16),
    /// mode, number of address bits (5 bits)
    SendAddr(SpimMode, u8),
    /// number of cycles (5 bits)
    Dummy(u8),
    /// Wait on an event. Note EventChannel coding needs interpretation prior to use.
    /// type of wait, channel, cycle count
    Wait(SpimWaitType),
    /// mode, words per xfer, bits per word, endianness, number of words to send
    TxData(SpimMode, SpimWordsPerXfer, u8, SpimEndian, u32),
    /// mode, words per xfer, bits per word, endianness, number of words to receive
    RxData(SpimMode, SpimWordsPerXfer, u8, SpimEndian, u32),
    /// repeat count
    RepeatNextCmd(u16),
    EndXfer(SpimEventGen),
    EndRepeat,
    /// mode, use byte alignment, check type, size of comparison (4 bits), comparison data
    RxCheck(SpimMode, SpimByteAlign, SpimCheckType, u8, u16),
    /// words per xfer, bits per word, endianness, number of words to receive; mode is always in 1-bit SPI
    FullDuplex(SpimWordsPerXfer, u8, SpimEndian, u32),
}
impl Into<u32> for SpimCmd {
    fn into(self) -> u32 {
        match self {
            SpimCmd::Config(pol, pha, div) => 0 << 28 | (pol as u32) << 9 | (pha as u32) << 8 | div as u32,
            SpimCmd::StartXfer(cs) => 1 << 28 | cs as u32,
            SpimCmd::SendCmd(mode, size, cmd) => {
                2 << 28 | (mode as u32) << 27 | ((size - 1) as u32 & 0x1F) << 16 | cmd as u32
            }
            SpimCmd::SendAddr(mode, size) => 3 << 28 | (mode as u32) << 27 | (size as u32 & 0x1F) << 16,
            SpimCmd::Dummy(cycles) => 4 << 28 | (cycles as u32 & 0x1F) << 16,
            SpimCmd::Wait(wait_type) => {
                let wait_code = match wait_type {
                    SpimWaitType::Event(EventChannel::Channel0) => 0,
                    SpimWaitType::Event(EventChannel::Channel1) => 1,
                    SpimWaitType::Event(EventChannel::Channel2) => 2,
                    SpimWaitType::Event(EventChannel::Channel3) => 3,
                    SpimWaitType::Cycles(cyc) => cyc as u32 | 0x1_00,
                };
                5 << 28 | wait_code
            }
            SpimCmd::TxData(mode, words_per_xfer, bits_per_word, endian, len) => {
                6 << 28
                    | (mode as u32) << 27
                    | ((words_per_xfer as u32) & 0x3) << 21
                    | (bits_per_word as u32 - 1) << 16
                    | (len as u32 - 1)
                    | (endian as u32) << 26
            }
            SpimCmd::RxData(mode, words_per_xfer, bits_per_word, endian, len) => {
                7 << 28
                    | (mode as u32) << 27
                    | ((words_per_xfer as u32) & 0x3) << 21
                    | (bits_per_word as u32 - 1) << 16
                    | (len as u32 - 1)
                    | (endian as u32) << 26
            }
            SpimCmd::RepeatNextCmd(count) => 8 << 28 | count as u32,
            SpimCmd::EndXfer(event) => 9 << 28 | event as u32,
            SpimCmd::EndRepeat => 10 << 28,
            SpimCmd::RxCheck(mode, align, check_type, size, data) => {
                11 << 28
                    | (mode as u32) << 27
                    | (align as u32) << 26
                    | (check_type as u32) << 24
                    | (size as u32 & 0xF) << 16
                    | data as u32
            }
            SpimCmd::FullDuplex(words_per_xfer, bits_per_word, endian, len) => {
                12 << 28
                    | ((words_per_xfer as u32) & 0x3) << 21
                    | (bits_per_word as u32 - 1) << 16
                    | (len as u32 - 1)
                    | (endian as u32) << 26
            }
        }
    }
}

#[derive(Debug)]
pub struct Spim {
    csr: CSR<u32>,
    cs: SpimCs,
    sot_wait: u8,
    eot_wait: u8,
    event_channel: Option<EventChannel>,
    mode: SpimMode,
    _align: SpimByteAlign,
    pub ifram: IframRange,
    // starts at the base of ifram range
    pub tx_buf_len_bytes: usize,
    // immediately after the tx buf len
    pub rx_buf_len_bytes: usize,
    dummy_cycles: u8,
    endianness: SpimEndian,
    // length of a pending txrx, if any
    pending_txrx: Option<usize>,
}

// length of the command buffer
const SPIM_CMD_BUF_LEN_BYTES: usize = 16;

impl Udma for Spim {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}

impl Spim {
    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    ///
    /// Return: the function can return None if it can't allocate enough memory
    /// for the requested tx/rx length.
    #[cfg(feature = "std")]
    pub unsafe fn new(
        channel: SpimChannel,
        spi_clk_freq: u32,
        sys_clk_freq: u32,
        pol: SpimClkPol,
        pha: SpimClkPha,
        chip_select: SpimCs,
        // cycles to wait between CS assert and data start
        sot_wait: u8,
        // cycles to wait after data stop and CS de-assert
        eot_wait: u8,
        event_channel: Option<EventChannel>,
        max_tx_len_bytes: usize,
        max_rx_len_bytes: usize,
        dummy_cycles: Option<u8>,
        mode: Option<SpimMode>,
    ) -> Option<Self> {
        let mut reqlen = max_tx_len_bytes + max_rx_len_bytes + SPIM_CMD_BUF_LEN_BYTES;
        if reqlen % 4096 != 0 {
            // round up to the nearest page size
            reqlen = (reqlen + 4096) & !4095;
        }
        if let Some(ifram) = IframRange::request(reqlen, None) {
            Some(Spim::new_with_ifram(
                channel,
                spi_clk_freq,
                sys_clk_freq,
                pol,
                pha,
                chip_select,
                sot_wait,
                eot_wait,
                event_channel,
                max_tx_len_bytes,
                max_rx_len_bytes,
                dummy_cycles,
                mode,
                ifram,
            ))
        } else {
            None
        }
    }

    /// This function is `unsafe` because it can only be called after the
    /// global shared UDMA state has been set up to un-gate clocks and set up
    /// events.
    ///
    /// It is also `unsafe` on Drop because you have to remember to unmap
    /// the clock manually as well once the object is dropped...
    ///
    /// Return: the function can return None if it can't allocate enough memory
    /// for the requested tx/rx length.
    pub unsafe fn new_with_ifram(
        channel: SpimChannel,
        spi_clk_freq: u32,
        sys_clk_freq: u32,
        pol: SpimClkPol,
        pha: SpimClkPha,
        chip_select: SpimCs,
        // cycles to wait between CS assert and data start
        sot_wait: u8,
        // cycles to wait after data stop and CS de-assert
        eot_wait: u8,
        event_channel: Option<EventChannel>,
        max_tx_len_bytes: usize,
        max_rx_len_bytes: usize,
        dummy_cycles: Option<u8>,
        mode: Option<SpimMode>,
        ifram: IframRange,
    ) -> Self {
        // this is a hardware limit - the DMA pointer is only this long!
        assert!(max_tx_len_bytes < 65536);
        assert!(max_rx_len_bytes < 65536);
        // now setup the channel
        let base_addr = match channel {
            SpimChannel::Channel0 => utra::udma_spim_0::HW_UDMA_SPIM_0_BASE,
            SpimChannel::Channel1 => utra::udma_spim_1::HW_UDMA_SPIM_1_BASE,
            SpimChannel::Channel2 => utra::udma_spim_2::HW_UDMA_SPIM_2_BASE,
            SpimChannel::Channel3 => utra::udma_spim_3::HW_UDMA_SPIM_3_BASE,
        };
        #[cfg(target_os = "xous")]
        let csr_range = xous::syscall::map_memory(
            xous::MemoryAddress::new(base_addr),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map serial port");
        #[cfg(target_os = "xous")]
        let csr = CSR::new(csr_range.as_mut_ptr() as *mut u32);
        #[cfg(not(target_os = "xous"))]
        let csr = CSR::new(base_addr as *mut u32);

        let clk_div = sys_clk_freq / (2 * spi_clk_freq);
        // make this a hard panic -- you'll find out at runtime that you f'd up
        // but at least you find out.
        assert!(clk_div < 256, "SPI clock divider is out of range");

        let mut spim = Spim {
            csr,
            cs: chip_select,
            sot_wait,
            eot_wait,
            event_channel,
            _align: SpimByteAlign::Disable,
            mode: mode.unwrap_or(SpimMode::Standard),
            ifram,
            tx_buf_len_bytes: max_tx_len_bytes,
            rx_buf_len_bytes: max_rx_len_bytes,
            dummy_cycles: dummy_cycles.unwrap_or(0),
            endianness: SpimEndian::MsbFirst,
            pending_txrx: None,
        };
        // setup the interface using a UDMA command
        spim.send_cmd_list(&[SpimCmd::Config(pol, pha, clk_div as u8)]);

        spim
    }

    /// For creating a clone to the current SPIM handle passed through a thread.
    ///
    /// Safety: can only be used on devices that are static for the life of the OS. Also, does nothing
    /// to prevent races/contention for the underlying device. The main reason this is introduced is
    /// to facilitate a panic handler for the graphics frame buffer, where we're about to kill the OS
    /// anyways: we don't care about soundness guarantees after this point.
    ///
    /// Note that the endianness is set to MSB first by default.
    pub unsafe fn from_raw_parts(
        csr: usize,
        cs: SpimCs,
        sot_wait: u8,
        eot_wait: u8,
        event_channel: Option<EventChannel>,
        mode: SpimMode,
        _align: SpimByteAlign,
        ifram: IframRange,
        tx_buf_len_bytes: usize,
        rx_buf_len_bytes: usize,
        dummy_cycles: u8,
    ) -> Self {
        Spim {
            csr: CSR::new(csr as *mut u32),
            cs,
            sot_wait,
            eot_wait,
            event_channel,
            _align,
            mode,
            ifram,
            tx_buf_len_bytes,
            rx_buf_len_bytes,
            dummy_cycles,
            endianness: SpimEndian::MsbFirst,
            pending_txrx: None,
        }
    }

    /// Blows a SPIM structure into parts that can be sent across a thread boundary.
    ///
    /// Safety: this is only safe because the *mut u32 for the CSR doesn't change, because it's tied to
    /// a piece of hardware, not some arbitrary block of memory.
    pub unsafe fn into_raw_parts(
        &self,
    ) -> (usize, SpimCs, u8, u8, Option<EventChannel>, SpimMode, SpimByteAlign, IframRange, usize, usize, u8)
    {
        (
            self.csr.base() as usize,
            self.cs,
            self.sot_wait,
            self.eot_wait,
            self.event_channel,
            self.mode,
            self._align,
            IframRange {
                phys_range: self.ifram.phys_range,
                virt_range: self.ifram.virt_range,
                conn: self.ifram.conn,
            },
            self.tx_buf_len_bytes,
            self.rx_buf_len_bytes,
            self.dummy_cycles,
        )
    }

    /// Note that endianness is disregarded in the case that the channel is being used to talk to
    /// a memory device, because the endianness is always MsbFirst.
    pub fn set_endianness(&mut self, endianness: SpimEndian) { self.endianness = endianness; }

    pub fn get_endianness(&self) -> SpimEndian { self.endianness }

    /// The command buf is *always* a `u32`; so tie the type down here.
    fn cmd_buf_mut(&mut self) -> &mut [u32] {
        &mut self.ifram.as_slice_mut()[(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<u32>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes + SPIM_CMD_BUF_LEN_BYTES) / size_of::<u32>()]
    }

    unsafe fn cmd_buf_phys(&self) -> &[u32] {
        &self.ifram.as_phys_slice()[(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<u32>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes + SPIM_CMD_BUF_LEN_BYTES) / size_of::<u32>()]
    }

    pub fn rx_buf<T: UdmaWidths>(&self) -> &[T] {
        &self.ifram.as_slice()[(self.tx_buf_len_bytes) / size_of::<T>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<T>()]
    }

    pub unsafe fn rx_buf_phys<T: UdmaWidths>(&self) -> &[T] {
        &self.ifram.as_phys_slice()[(self.tx_buf_len_bytes) / size_of::<T>()
            ..(self.tx_buf_len_bytes + self.rx_buf_len_bytes) / size_of::<T>()]
    }

    pub fn tx_buf_mut<T: UdmaWidths>(&mut self) -> &mut [T] {
        &mut self.ifram.as_slice_mut()[..self.tx_buf_len_bytes / size_of::<T>()]
    }

    pub unsafe fn tx_buf_phys<T: UdmaWidths>(&self) -> &[T] {
        &self.ifram.as_phys_slice()[..self.tx_buf_len_bytes / size_of::<T>()]
    }

    fn send_cmd_list(&mut self, cmds: &[SpimCmd]) {
        for cmd_chunk in cmds.chunks(SPIM_CMD_BUF_LEN_BYTES / size_of::<u32>()) {
            for (src, dst) in cmd_chunk.iter().zip(self.cmd_buf_mut().iter_mut()) {
                *dst = (*src).into();
            }
            // safety: this is safe because the cmd_buf_phys() slice is passed to a function that only
            // uses it as a base/bounds reference and it will not actually access the data.
            unsafe {
                self.udma_enqueue(
                    Bank::Custom,
                    &self.cmd_buf_phys()[..cmd_chunk.len()],
                    CFG_EN | CFG_SIZE_32,
                );
            }
        }
    }

    pub fn is_tx_busy(&self) -> bool { self.udma_busy(Bank::Tx) || self.udma_busy(Bank::Custom) }

    pub fn tx_data_await(&self, _use_yield: bool) {
        while self.is_tx_busy() {
            #[cfg(feature = "std")]
            if _use_yield {
                xous::yield_slice();
            }
        }
    }

    /// `tx_data_async` will queue a data buffer into the SPIM interface and return as soon as the enqueue
    /// is completed (which can be before the transmission is actually done). The function may partially
    /// block, however, if the size of the buffer to be sent is larger than the largest allowable DMA
    /// transfer. In this case, it will block until the last chunk that can be transferred without
    /// blocking.
    pub fn tx_data_async<T: UdmaWidths + Copy>(&mut self, data: &[T], use_cs: bool, eot_event: bool) {
        unsafe {
            self.tx_data_async_inner(Some(data), None, use_cs, eot_event);
        }
    }

    /// `tx_data_async_from_parts` does a similar function as `tx_data_async`, but it expects that the
    /// data to send is already copied into the DMA buffer. In this case, no copying is done, and the
    /// `(start, len)` pair is used to specify the beginning and the length of the data to send that is
    /// already resident in the DMA buffer.
    ///
    /// Safety:
    ///   - Only safe to use when the data has already been copied into the DMA buffer, and the size and len
    ///     fields are within bounds.
    pub unsafe fn tx_data_async_from_parts<T: UdmaWidths + Copy>(
        &mut self,
        start: usize,
        len: usize,
        use_cs: bool,
        eot_event: bool,
    ) {
        self.tx_data_async_inner(None::<&[T]>, Some((start, len)), use_cs, eot_event);
    }

    /// This is the inner implementation of the two prior calls. A lot of the boilerplate is the same,
    /// the main difference is just whether the passed data shall be copied or not.
    ///
    /// Panics: Panics if both `data` and `parts` are `None`. If both are `Some`, `data` will take precedence.
    unsafe fn tx_data_async_inner<T: UdmaWidths + Copy>(
        &mut self,
        data: Option<&[T]>,
        parts: Option<(usize, usize)>,
        use_cs: bool,
        eot_event: bool,
    ) {
        let bits_per_xfer = size_of::<T>() * 8;
        let total_words = if let Some(data) = data {
            data.len()
        } else if let Some((_start, len)) = parts {
            len
        } else {
            // I can't figure out how to wrap a... &[T] in an enum? A slice of a type of trait
            // seems to need some sort of `dyn` keyword plus other stuff that is a bit heavy for
            // a function that is private (note the visibility on this function). Handling this
            // instead with a runtime check-to-panic.
            panic!("Inner function was set up with incorrect arguments");
        };
        let mut words_sent: usize = 0;

        if use_cs {
            // ensure that we can clobber the command list storage
            while self.udma_busy(Bank::Custom) || self.udma_busy(Bank::Tx) {}
            if self.sot_wait == 0 {
                self.send_cmd_list(&[SpimCmd::StartXfer(self.cs)])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::StartXfer(self.cs),
                    SpimCmd::Wait(SpimWaitType::Cycles(self.sot_wait)),
                ])
            }
            // wait for CS to assert
            while self.udma_busy(Bank::Custom) {}
        }
        let mut one_shot = false;
        let evt = if eot_event { SpimEventGen::Enabled } else { SpimEventGen::Disabled };
        while words_sent < total_words {
            // determine the valid length of data we could send
            let tx_len = (total_words - words_sent).min(self.tx_buf_len_bytes);
            // setup the command list for data to send
            let cmd_list_oneshot = [
                SpimCmd::TxData(
                    self.mode,
                    SpimWordsPerXfer::Words1,
                    bits_per_xfer as u8,
                    self.get_endianness(),
                    tx_len as u32,
                ),
                SpimCmd::EndXfer(evt),
            ];
            let cmd_list_repeated = [SpimCmd::TxData(
                self.mode,
                SpimWordsPerXfer::Words1,
                bits_per_xfer as u8,
                self.get_endianness(),
                tx_len as u32,
            )];
            if tx_len == total_words && use_cs {
                one_shot = true;
                self.send_cmd_list(&cmd_list_oneshot);
            } else {
                self.send_cmd_list(&cmd_list_repeated);
            }
            let cfg_size = match size_of::<T>() {
                1 => CFG_SIZE_8,
                2 => CFG_SIZE_16,
                4 => CFG_SIZE_32,
                _ => panic!("Illegal size of UdmaWidths: should not be possible"),
            };
            if let Some(data) = data {
                for (src, dst) in
                    data[words_sent..words_sent + tx_len].iter().zip(self.tx_buf_mut().iter_mut())
                {
                    *dst = *src;
                }
                // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
                unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<T>()[..tx_len], CFG_EN | cfg_size) }
            } else if let Some((start, _len)) = parts {
                // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
                // This will correctly panic if the size of the data to be sent is larger than the physical
                // tx_buf.
                unsafe {
                    self.udma_enqueue(
                        Bank::Tx,
                        &self.tx_buf_phys::<T>()[(start + words_sent)..(start + words_sent + tx_len)],
                        CFG_EN | cfg_size,
                    )
                }
            } // the else clause "shouldn't happen" because of the runtime check up top!
            words_sent += tx_len;

            // wait until the transfer is done before doing the next iteration, if there is a next iteration
            // last iteration falls through without waiting...
            if words_sent < total_words {
                while self.udma_busy(Bank::Tx) {
                    #[cfg(feature = "std")]
                    xous::yield_slice();
                }
            }
        }
        if use_cs && !one_shot {
            // wait for all data to transmit before de-asserting CS
            while self.udma_busy(Bank::Tx) {}
            if self.eot_wait == 0 {
                self.send_cmd_list(&[SpimCmd::EndXfer(evt)])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::Wait(SpimWaitType::Cycles(self.eot_wait)),
                    SpimCmd::EndXfer(evt),
                ])
            }
            while self.udma_busy(Bank::Custom) {}
        }
    }

    /// Wait for the pending tx/rx cycle to finish, returns a pointer to the Rx buffer when done.
    pub fn txrx_await(&mut self, _use_yield: bool) -> Result<&[u8], xous::Error> {
        #[cfg(not(target_os = "xous"))]
        if let Some(pending) = self.pending_txrx.take() {
            while self.udma_busy(Bank::Tx) || self.udma_busy(Bank::Rx) || self.udma_busy(Bank::Custom) {}
            Ok(&self.rx_buf()[..pending])
        } else {
            Err(xous::Error::UseBeforeInit)
        }
        #[cfg(target_os = "xous")]
        {
            if let Some(pending) = self.pending_txrx.take() {
                let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
                let start = tt.elapsed_ms();
                let mut now = tt.elapsed_ms();
                const TIMEOUT_MS: u64 = 500;
                while (self.udma_busy(Bank::Tx) || self.udma_busy(Bank::Rx) || self.udma_busy(Bank::Custom))
                    && ((now - start) < TIMEOUT_MS)
                {
                    now = tt.elapsed_ms();
                }
                if now - start >= TIMEOUT_MS {
                    log::warn!(
                        "Timeout in txrx_await(): Tx {:?} Rx {:?} Custom {:?}, Rx SA: {:x}, Rx Cfg: {:x}",
                        self.udma_busy(Bank::Tx),
                        self.udma_busy(Bank::Rx),
                        self.udma_busy(Bank::Custom),
                        unsafe {
                            self.csr().base().add(Bank::Rx as usize).add(DmaReg::Saddr.into()).read_volatile()
                        },
                        unsafe {
                            self.csr().base().add(Bank::Rx as usize).add(DmaReg::Cfg.into()).read_volatile()
                        },
                    );
                    unsafe {
                        self.csr()
                            .base()
                            .add(Bank::Rx as usize)
                            .add(DmaReg::Cfg.into())
                            .write_volatile(CFG_CLEAR);
                        log::info!(
                            "Rx Cfg: {:x}",
                            self.csr().base().add(Bank::Rx as usize).add(DmaReg::Cfg.into()).read_volatile()
                        );
                        self.csr().base().add(Bank::Rx as usize).add(DmaReg::Saddr.into()).write_volatile(0);
                        self.csr().base().add(Bank::Rx as usize).add(DmaReg::Cfg.into()).write_volatile(0); // clear bit is not self-clearing
                    };
                }
                Ok(&self.rx_buf()[..pending])
            } else {
                Err(xous::Error::UseBeforeInit)
            }
        }
    }

    /// `txrx_data_async` will return as soon as all the pending operations are queued. This will error
    /// out if the slice to be transmitted would not fit into the existing buffer. To read the receive
    /// data, call `txrx_await` to get a pointer to the rx buffer, which is only granted after the
    /// transaction has finished running.
    pub fn txrx_data_async<T: UdmaWidths + Copy>(
        &mut self,
        data: &[T],
        use_cs: bool,
        eot_event: bool,
    ) -> Result<(), xous::Error> {
        unsafe { self.txrx_data_async_inner(Some(data), None, use_cs, eot_event) }
    }

    /// `txrx_data_async_from_parts` does a similar function as `txrx_data_async`, but it expects that the
    /// data to send is already copied into the DMA buffer. In this case, no copying is done, and the
    /// `(start, len)` pair is used to specify the beginning and the length of the data to send that is
    /// already resident in the DMA buffer.
    ///
    /// Safety:
    ///   - Only safe to use when the data has already been copied into the DMA buffer, and the size and len
    ///     fields are within bounds.
    pub unsafe fn txrx_data_async_from_parts<T: UdmaWidths + Copy>(
        &mut self,
        start: usize,
        len: usize,
        use_cs: bool,
        eot_event: bool,
    ) -> Result<(), xous::Error> {
        self.txrx_data_async_inner(None::<&[T]>, Some((start, len)), use_cs, eot_event)
    }

    /// This is the inner implementation of the two prior calls. A lot of the boilerplate is the same,
    /// the main difference is just whether the passed data shall be copied or not.
    ///
    /// Panics: Panics if both `data` and `parts` are `None`. If both are `Some`, `data` will take precedence.
    unsafe fn txrx_data_async_inner<T: UdmaWidths + Copy>(
        &mut self,
        data: Option<&[T]>,
        parts: Option<(usize, usize)>,
        use_cs: bool,
        eot_event: bool,
    ) -> Result<(), xous::Error> {
        if self.pending_txrx.is_some() {
            // block until the prior transaction is done
            self.txrx_await(false).ok();
        }
        let bits_per_xfer = size_of::<T>() * 8;
        let tx_len = if let Some(data) = data {
            data.len() * size_of::<T>()
        } else if let Some((_start, len)) = parts {
            len * size_of::<T>()
        } else {
            // I can't figure out how to wrap a... &[T] in an enum? A slice of a type of trait
            // seems to need some sort of `dyn` keyword plus other stuff that is a bit heavy for
            // a function that is private (note the visibility on this function). Handling this
            // instead with a runtime check-to-panic.
            panic!("Inner function was set up with incorrect arguments");
        };
        if tx_len > self.tx_buf_len_bytes || tx_len > self.rx_buf_len_bytes {
            return Err(xous::Error::OutOfMemory);
        }
        self.pending_txrx = Some(tx_len);

        let evt = if eot_event { SpimEventGen::Enabled } else { SpimEventGen::Disabled };
        if use_cs {
            if self.sot_wait == 0 {
                self.send_cmd_list(&[
                    SpimCmd::StartXfer(self.cs),
                    SpimCmd::FullDuplex(
                        SpimWordsPerXfer::Words1,
                        bits_per_xfer as u8,
                        self.get_endianness(),
                        tx_len as u32,
                    ),
                    SpimCmd::EndXfer(evt),
                ])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::StartXfer(self.cs),
                    SpimCmd::Wait(SpimWaitType::Cycles(self.sot_wait)),
                    SpimCmd::FullDuplex(
                        SpimWordsPerXfer::Words1,
                        bits_per_xfer as u8,
                        self.get_endianness(),
                        tx_len as u32,
                    ),
                    SpimCmd::EndXfer(evt),
                ])
            }
        } else {
            if self.sot_wait == 0 {
                self.send_cmd_list(&[SpimCmd::FullDuplex(
                    SpimWordsPerXfer::Words1,
                    bits_per_xfer as u8,
                    self.get_endianness(),
                    tx_len as u32,
                )])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::Wait(SpimWaitType::Cycles(self.sot_wait)),
                    SpimCmd::FullDuplex(
                        SpimWordsPerXfer::Words1,
                        bits_per_xfer as u8,
                        self.get_endianness(),
                        tx_len as u32,
                    ),
                ])
            }
        }

        let cfg_size = match size_of::<T>() {
            1 => CFG_SIZE_8,
            2 => CFG_SIZE_16,
            4 => CFG_SIZE_32,
            _ => panic!("Illegal size of UdmaWidths: should not be possible"),
        };
        if let Some(data) = data {
            for (src, dst) in data[..tx_len].iter().zip(self.tx_buf_mut().iter_mut()) {
                *dst = *src;
            }
            // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
            unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<T>()[..tx_len], CFG_EN | cfg_size) };
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &self.rx_buf_phys::<T>()[..tx_len],
                    CFG_EN | cfg_size | CFG_BACKPRESSURE,
                )
            }
        } else if let Some((start, _len)) = parts {
            // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
            // This will correctly panic if the size of the data to be sent is larger than the physical
            // tx_buf.
            unsafe {
                self.udma_enqueue(
                    Bank::Tx,
                    &self.tx_buf_phys::<T>()[start..(start + tx_len)],
                    CFG_EN | cfg_size,
                );
                self.udma_enqueue(
                    Bank::Rx,
                    &self.rx_buf_phys::<T>()[..tx_len],
                    CFG_EN | cfg_size | CFG_BACKPRESSURE,
                )
            }
        }
        Ok(())
    }

    /// This is waiting for a test target to test against.
    pub fn rx_data<T: UdmaWidths + Copy>(&mut self, _rx_data: &mut [T], _cs: Option<SpimCs>) {
        todo!("Not yet done...can template off of txrx data, eliminating the Tx requirement");
    }

    /// Activate is the logical sense, not the physical sense. To be clear: `true` causes CS to go low.
    fn mem_cs(&mut self, activate: bool) {
        if activate {
            if self.sot_wait == 0 {
                self.send_cmd_list(&[SpimCmd::StartXfer(self.cs)])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::StartXfer(self.cs),
                    SpimCmd::Wait(SpimWaitType::Cycles(self.sot_wait)),
                ])
            }
        } else {
            let evt =
                if self.event_channel.is_some() { SpimEventGen::Enabled } else { SpimEventGen::Disabled };
            if self.eot_wait == 0 {
                self.send_cmd_list(&[SpimCmd::EndXfer(evt)])
            } else {
                self.send_cmd_list(&[
                    SpimCmd::Wait(SpimWaitType::Cycles(self.eot_wait)),
                    SpimCmd::EndXfer(evt),
                ])
            }
        }
    }

    fn mem_send_cmd(&mut self, cmd: u8) {
        let cmd_list = [SpimCmd::SendCmd(self.mode, 8, cmd as u16)];
        self.send_cmd_list(&cmd_list);
        while self.udma_busy(Bank::Custom) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
    }

    pub fn mem_read_id_flash(&mut self) -> u32 {
        self.mem_cs(true);

        // send the RDID command
        match self.mode {
            SpimMode::Standard => self.mem_send_cmd(0x9F),
            SpimMode::Quad => self.mem_send_cmd(0xAF),
        }

        // read back the ID result
        let cmd_list = [SpimCmd::RxData(self.mode, SpimWordsPerXfer::Words1, 8, SpimEndian::MsbFirst, 3)];
        // safety: this is safe because rx_buf_phys() slice is only used as a base/bounds reference
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.rx_buf_phys::<u8>()[..3],
                CFG_EN | CFG_SIZE_8 | CFG_BACKPRESSURE,
            )
        };
        self.send_cmd_list(&cmd_list);
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }

        let ret = u32::from_le_bytes([self.rx_buf()[0], self.rx_buf()[1], self.rx_buf()[2], 0x0]);

        self.mem_cs(false);
        ret
    }

    /// Side-effects: unsets QPI mode if it was previously set
    pub fn mem_read_id_ram(&mut self) -> u32 {
        self.mem_cs(true);

        // send the RDID command
        self.mem_send_cmd(0x9F);

        // read back the ID result
        // The ID requires 24 bits "dummy" address field, then followed by 2 bytes ID + KGD, and then
        // 48 bits of unique ID -- we only retrieve the top 16 of that here.
        let cmd_list = [SpimCmd::RxData(self.mode, SpimWordsPerXfer::Words1, 8, SpimEndian::MsbFirst, 7)];
        // safety: this is safe because rx_buf_phys() slice is only used as a base/bounds reference
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.rx_buf_phys::<u8>()[..7],
                CFG_EN | CFG_SIZE_8 | CFG_BACKPRESSURE,
            )
        };
        self.send_cmd_list(&cmd_list);
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }

        let ret =
            u32::from_le_bytes([self.rx_buf()[3], self.rx_buf()[4], self.rx_buf()[5], self.rx_buf()[6]]);

        self.mem_cs(false);
        ret
    }

    pub fn mem_qpi_mode(&mut self, activate: bool) {
        self.mem_cs(true);
        if activate {
            self.mem_send_cmd(0x35);
        } else {
            self.mode = SpimMode::Quad; // pre-assumes quad mode
            self.mem_send_cmd(0xF5);
        }
        self.mem_cs(false);
        // change the mode only after the command has been sent
        if activate {
            self.mode = SpimMode::Quad;
        } else {
            self.mode = SpimMode::Standard;
        }
    }

    /// Side-effects: unsets QPI mode if it was previously set
    /// TODO: this does not seem to work. Setting it causes some strange behaviors
    /// on reads (but QE mode is enabled, so something must have worked). This
    /// needs to be looked into more. Oddly enough, it looks "fine" on the logic
    /// analyzer when I checked it early on, but obviously something is not right.
    pub fn mem_write_status_register(&mut self, status: u8, config: u8) {
        if self.mode != SpimMode::Standard {
            self.mem_qpi_mode(false);
        }
        self.mem_cs(true);
        self.mem_send_cmd(0x1);
        // setup the command list for data to send
        let cmd_list =
            [SpimCmd::TxData(self.mode, SpimWordsPerXfer::Words1, 8 as u8, SpimEndian::MsbFirst, 2 as u32)];
        self.tx_buf_mut()[..2].copy_from_slice(&[status, config]);
        // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
        unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..2], CFG_EN | CFG_SIZE_8) }
        self.send_cmd_list(&cmd_list);

        while self.udma_busy(Bank::Tx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        self.mem_cs(false);
    }

    /// Note that `use_yield` is disallowed in interrupt contexts (e.g. swapper)
    pub fn mem_read(&mut self, addr: u32, buf: &mut [u8], _use_yield: bool) -> bool {
        // divide into buffer-sized chunks + repeat cycle on each buffer increment
        // this is because the size of the buffer is meant to represent the limit of the
        // target device's memory page (i.e., the point at which you'd wrap when reading)
        let mut offset = 0;
        let mut timeout = 0;
        let mut success = true;
        for chunk in buf.chunks_mut(self.rx_buf_len_bytes) {
            let chunk_addr = addr as usize + offset;
            let addr_plus_dummy = (24 / 8) + self.dummy_cycles / 2;
            let cmd_list = [
                SpimCmd::SendCmd(self.mode, 8, 0xEB),
                SpimCmd::TxData(
                    self.mode,
                    SpimWordsPerXfer::Words1,
                    8 as u8,
                    SpimEndian::MsbFirst,
                    addr_plus_dummy as u32,
                ),
            ];
            let a = chunk_addr.to_be_bytes();
            self.tx_buf_mut()[..3].copy_from_slice(&[a[1], a[2], a[3]]);
            // the remaining bytes are junk
            self.tx_buf_mut()[3..6].copy_from_slice(&[0xFFu8, 0xFFu8, 0xFFu8]);
            self.mem_cs(true);
            // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
            unsafe {
                self.udma_enqueue(
                    Bank::Tx,
                    &self.tx_buf_phys::<u8>()[..addr_plus_dummy as usize],
                    CFG_EN | CFG_SIZE_8,
                )
            }
            self.send_cmd_list(&cmd_list);
            let rd_cmd = [SpimCmd::RxData(
                self.mode,
                SpimWordsPerXfer::Words1,
                8,
                SpimEndian::MsbFirst,
                chunk.len() as u32,
            )];
            while self.udma_busy(Bank::Tx) {
                #[cfg(feature = "std")]
                if _use_yield {
                    xous::yield_slice();
                }
            }
            // safety: this is safe because rx_buf_phys() slice is only used as a base/bounds reference
            unsafe {
                self.udma_enqueue(
                    Bank::Rx,
                    &self.rx_buf_phys::<u8>()[..chunk.len()],
                    CFG_EN | CFG_SIZE_8 | CFG_BACKPRESSURE,
                )
            };
            self.send_cmd_list(&rd_cmd);
            while self.udma_busy(Bank::Rx) {
                // TODO: figure out why this timeout detection code is necessary.
                // It seems that some traffic during the UDMA access can cause the UDMA
                // engine to hang. For example, if we put a dcache_flush() routine in this
                // loop, it will fail immediately. This might be something to look into
                // in simulation.
                timeout += 1;
                if (self.mode == SpimMode::Quad) && (timeout > chunk.len() * 10_000) {
                    success = false;
                    // unsuccessful attempt to clear the pending transfer manually
                    // the root cause of this is when the UDMA RX FIFO fills up and
                    // RX packets get dropped. The Rx counter becomes "desynced" from the
                    // data stream, and thus it never hits 0 (it wraps around and goes negative).
                    // The code below does not recover the PHY into a usable state.
                    unsafe {
                        self.csr()
                            .base()
                            .add(Bank::Rx as usize)
                            .add(DmaReg::Cfg.into())
                            .write_volatile(CFG_SIZE_8 | CFG_CLEAR);
                        self.csr().base().add(Bank::Rx as usize).add(DmaReg::Saddr.into()).write_volatile(0);
                        self.csr()
                            .base()
                            .add(Bank::Rx as usize)
                            .add(DmaReg::Cfg.into())
                            .write_volatile(CFG_SIZE_8); // clear bit is not self-clearing
                    };
                    /*
                    // send an EOT
                    let cmd_list = [SpimCmd::EndXfer(SpimEventGen::Disabled)];
                    self.send_cmd_list(&cmd_list);
                    crate::println!(
                        "udma timeout: cfg {:x}, saddr {:x}",
                        unsafe {
                            self.csr().base().add(Bank::Rx as usize).add(DmaReg::Cfg.into()).read_volatile()
                        },
                        unsafe {
                            self.csr().base().add(Bank::Rx as usize).add(DmaReg::Saddr.into()).read_volatile()
                        }
                    );
                    */
                    break;
                }
                #[cfg(feature = "std")]
                if _use_yield {
                    xous::yield_slice();
                }
            }
            self.mem_cs(false);
            chunk.copy_from_slice(&self.rx_buf()[..chunk.len()]);
            offset += chunk.len();
        }
        success
    }

    /// This should only be called on SPI RAM -- not valid for FLASH devices, they need a programming routine!
    /// Note that `use_yield` is disallowed in interrupt contexts
    pub fn mem_ram_write(&mut self, addr: u32, buf: &[u8], _use_yield: bool) {
        // divide into buffer-sized chunks + repeat cycle on each buffer increment
        // this is because the size of the buffer is meant to represent the limit of the
        // target device's memory page (i.e., the point at which you'd wrap when reading)
        let mut offset = 0;
        for chunk in buf.chunks(self.tx_buf_len_bytes) {
            let chunk_addr = addr as usize + offset;
            let cmd_list = [
                SpimCmd::SendCmd(self.mode, 8, 0x38),
                SpimCmd::TxData(self.mode, SpimWordsPerXfer::Words1, 8 as u8, SpimEndian::MsbFirst, 3),
            ];
            let a = chunk_addr.to_be_bytes();
            self.tx_buf_mut()[..3].copy_from_slice(&[a[1], a[2], a[3]]);
            self.mem_cs(true);
            // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
            unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..3], CFG_EN | CFG_SIZE_8) }
            self.send_cmd_list(&cmd_list);
            let wr_cmd = [SpimCmd::TxData(
                self.mode,
                SpimWordsPerXfer::Words1,
                8,
                SpimEndian::MsbFirst,
                chunk.len() as u32,
            )];
            while self.udma_busy(Bank::Tx) {
                #[cfg(feature = "std")]
                if _use_yield {
                    xous::yield_slice();
                }
            }
            self.tx_buf_mut()[..chunk.len()].copy_from_slice(chunk);
            // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
            unsafe {
                self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..chunk.len()], CFG_EN | CFG_SIZE_8)
            };
            self.send_cmd_list(&wr_cmd);
            while self.udma_busy(Bank::Tx) {
                #[cfg(feature = "std")]
                if _use_yield {
                    xous::yield_slice();
                }
            }
            self.mem_cs(false);
            offset += chunk.len();
        }
    }

    fn flash_wren(&mut self) {
        self.mem_cs(true);
        self.mem_send_cmd(0x06);
        self.mem_cs(false);
    }

    fn flash_wrdi(&mut self) {
        self.mem_cs(true);
        self.mem_send_cmd(0x04);
        self.mem_cs(false);
    }

    fn flash_rdsr(&mut self) -> u8 {
        self.mem_cs(true);
        self.mem_send_cmd(0x05);
        let cmd_list = [SpimCmd::RxData(self.mode, SpimWordsPerXfer::Words1, 8, SpimEndian::MsbFirst, 1)];
        // safety: this is safe because rx_buf_phys() slice is only used as a base/bounds reference
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.rx_buf_phys::<u8>()[..1],
                CFG_EN | CFG_SIZE_8 | CFG_BACKPRESSURE,
            )
        };
        self.send_cmd_list(&cmd_list);
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        let ret = self.rx_buf()[0];

        self.mem_cs(false);
        ret
    }

    fn flash_rdscur(&mut self) -> u8 {
        self.mem_cs(true);
        self.mem_send_cmd(0x2b);
        let cmd_list = [SpimCmd::RxData(self.mode, SpimWordsPerXfer::Words1, 8, SpimEndian::MsbFirst, 1)];
        // safety: this is safe because rx_buf_phys() slice is only used as a base/bounds reference
        unsafe {
            self.udma_enqueue(
                Bank::Rx,
                &self.rx_buf_phys::<u8>()[0..1],
                CFG_EN | CFG_SIZE_8 | CFG_BACKPRESSURE,
            )
        };
        self.send_cmd_list(&cmd_list);
        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        let ret = self.rx_buf()[0];

        self.mem_cs(false);
        ret
    }

    fn flash_se(&mut self, sector_address: u32) {
        self.mem_cs(true);
        self.mem_send_cmd(0x20);
        let cmd_list =
            [SpimCmd::TxData(self.mode, SpimWordsPerXfer::Words1, 8 as u8, SpimEndian::MsbFirst, 3 as u32)];
        self.tx_buf_mut()[..3].copy_from_slice(&sector_address.to_be_bytes()[1..]);
        // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
        unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..3], CFG_EN | CFG_SIZE_8) }
        self.send_cmd_list(&cmd_list);

        while self.udma_busy(Bank::Tx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        self.mem_cs(false);
    }

    pub fn flash_erase_sector(&mut self, sector_address: u32) -> u8 {
        // enable writes: set wren mode
        // crate::println!("flash_erase_sector: {:x}", sector_address);
        loop {
            self.flash_wren();
            let status = self.flash_rdsr();
            // crate::println!("wren status: {:x}", status);
            if status & 0x02 != 0 {
                break;
            }
        }
        // issue erase command
        self.flash_se(sector_address);
        // wait for WIP bit to drop
        loop {
            let status = self.flash_rdsr();
            // crate::println!("WIP status: {:x}", status);
            if status & 0x01 == 0 {
                break;
            }
        }
        // get the success code for return
        let result = self.flash_rdscur();
        // disable writes: send wrdi
        if self.flash_rdsr() & 0x02 != 0 {
            loop {
                self.flash_wrdi();
                let status = self.flash_rdsr();
                // crate::println!("WRDI status: {:x}", status);
                if status & 0x02 == 0 {
                    break;
                }
            }
        }
        result
    }

    fn flash_be(&mut self, block_address: u32) {
        self.mem_cs(true);
        self.mem_send_cmd(0xd8);
        let cmd_list =
            [SpimCmd::TxData(self.mode, SpimWordsPerXfer::Words1, 8 as u8, SpimEndian::MsbFirst, 3 as u32)];
        self.tx_buf_mut()[..3].copy_from_slice(&block_address.to_be_bytes()[1..]);
        // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
        unsafe { self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..3], CFG_EN | CFG_SIZE_8) }
        self.send_cmd_list(&cmd_list);

        while self.udma_busy(Bank::Tx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        self.mem_cs(false);
    }

    pub fn flash_erase_block(&mut self, start: usize, len: usize) -> bool {
        if (start & (BLOCK_ERASE_LEN - 1)) != 0 {
            // log::warn!("Bulk erase start address is not block-aligned. Aborting.");
            return false;
        }
        if (len & (BLOCK_ERASE_LEN - 1)) != 0 {
            // log::warn!("Bulk erase end address is not block-aligned. Aborting.");
            return false;
        }
        for block_addr in (start..start + len).step_by(BLOCK_ERASE_LEN as usize) {
            // enable writes: set wren mode
            // crate::println!("flash_erase_sector: {:x}", sector_address);
            loop {
                self.flash_wren();
                let status = self.flash_rdsr();
                // crate::println!("wren status: {:x}", status);
                if status & 0x02 != 0 {
                    break;
                }
            }
            // issue erase command
            self.flash_be(block_addr as u32);
            // wait for WIP bit to drop
            loop {
                let status = self.flash_rdsr();
                // crate::println!("WIP status: {:x}", status);
                if status & 0x01 == 0 {
                    break;
                }
            }
            // get the success code for return
            // let result = self.flash_rdscur();
            // disable writes: send wrdi
            if self.flash_rdsr() & 0x02 != 0 {
                loop {
                    self.flash_wrdi();
                    let status = self.flash_rdsr();
                    // crate::println!("WRDI status: {:x}", status);
                    if status & 0x02 == 0 {
                        break;
                    }
                }
            }
        }
        true
    }

    /// This routine can data that is strictly a multiple of a page length (256 bytes)
    pub fn mem_flash_write_page(&mut self, addr: u32, buf: &[u8; FLASH_PAGE_LEN]) -> bool {
        // crate::println!("write_page: {:x}, {:x?}", addr, &buf[..8]);
        // enable writes: set wren mode
        loop {
            self.flash_wren();
            let status = self.flash_rdsr();
            // crate::println!("wren status: {:x}", status);
            if status & 0x02 != 0 {
                break;
            }
        }

        self.mem_cs(true);
        self.mem_send_cmd(0x02); // PP
        let cmd_list = [SpimCmd::TxData(
            self.mode,
            SpimWordsPerXfer::Words1,
            8 as u8,
            SpimEndian::MsbFirst,
            3 + FLASH_PAGE_LEN as u32,
        )];
        self.tx_buf_mut()[..3].copy_from_slice(&addr.to_be_bytes()[1..]);
        self.tx_buf_mut()[3..3 + FLASH_PAGE_LEN].copy_from_slice(buf);
        // safety: this is safe because tx_buf_phys() slice is only used as a base/bounds reference
        unsafe {
            self.udma_enqueue(Bank::Tx, &self.tx_buf_phys::<u8>()[..3 + FLASH_PAGE_LEN], CFG_EN | CFG_SIZE_8)
        }
        self.send_cmd_list(&cmd_list);

        while self.udma_busy(Bank::Tx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }
        self.mem_cs(false);

        loop {
            // wait while WIP is set
            let status = self.flash_rdsr();
            // crate::println!("WIP status: {:x}", status);
            if (status & 0x01) == 0 {
                break;
            }
        }
        // get the success code for return
        let result = self.flash_rdscur();
        if result & 0x20 != 0 {
            return false;
        }
        // disable writes: send wrdi
        if self.flash_rdsr() & 0x02 != 0 {
            loop {
                self.flash_wrdi();
                let status = self.flash_rdsr();
                // crate::println!("WRDI status: {:x}", status);
                if status & 0x02 == 0 {
                    break;
                }
            }
        }
        true
    }
}

// Stub retained because it is helpful for debugging some bus contention issues.
#[allow(dead_code)]
fn cache_flush() {
    unsafe {
        // let the write go through before continuing
        #[rustfmt::skip]
        core::arch::asm!(
            ".word 0x500F",
            "nop",
            "nop",
            "nop",
            "nop",
            "fence",
            "nop",
            "nop",
            "nop",
            "nop",
        );
    }
}
