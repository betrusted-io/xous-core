//! UDMA ADC block driver
//!
//! Supports internal temperature sensor and external voltage measurement on
//! PA04–PA07 via the uDMA RX channel.
//!
//! # `std` (Xous userspace)
//! ```no_run
//! let mut adc = unsafe { Adc::new(perclk_freq) };
//! let raw = adc.read_raw(AdcSource::Temperature);
//! let temp_c = Adc::raw_to_temp_celsius(raw as f64);
//!
//! let raw = adc.read_raw(AdcSource::Ext(AdcExtChannel::Pa04));
//! let volts = Adc::raw_to_voltage(raw);
//! ```
//!
//! # `no_std` (loader / bare-metal)
//! ```no_run
//! let mut adc = unsafe { Adc::new_baremetal(perclk_freq, dma_buf_phys) }; // no_std
//! ```

use utralib::utra::udma_adc;
use utralib::*;

use crate::ifram::IframRange;
use crate::udma::*;

// ============================================================================
// Fine-grained field definitions for REG_CR_ADC
//
// The auto-generated UTRA only exposes one monolithic 28-bit field
// (REG_CR_ADC_CR_ADC). We break it out here to match the datasheet's
// per-bit definitions for ADC_CR_CFG (byte offset 0x10, word offset 4).
// ============================================================================
pub mod cr_adc {
    use super::*;

    /// Bit[0]: Bandgap chopper enable (0 = disable, 1 = enable)
    pub const CHOPPER_EN: Field = Field::new(1, 0, udma_adc::REG_CR_ADC);
    /// Bit[1]: Temperature-related voltage buffer (0 = disable, 1 = enable)
    pub const TEMP_BUF_EN: Field = Field::new(1, 1, udma_adc::REG_CR_ADC);
    /// Bit[2]: Bandgap voltage buffer / reference select
    ///   0 = disable buffer, use AVDD as reference
    ///   1 = enable buffer, use bandgap voltage as reference
    pub const BANDGAP_BUF_EN: Field = Field::new(1, 2, udma_adc::REG_CR_ADC);
    /// Bit[3]: External voltage buffer (0 = disable, 1 = enable)
    pub const EXT_BUF_EN: Field = Field::new(1, 3, udma_adc::REG_CR_ADC);
    /// Bit[4]: Temperature-related voltage control (0 = voltage 1, 1 = voltage 2)
    pub const TEMP_V_CTRL: Field = Field::new(1, 4, udma_adc::REG_CR_ADC);
    /// Bit[5]: Temperature voltage filter bypass (0 = filter, 1 = bypass)
    pub const TEMP_FILTER_BYPASS: Field = Field::new(1, 5, udma_adc::REG_CR_ADC);
    /// Bit[6]: Bandgap voltage filter bypass (0 = filter, 1 = bypass)
    pub const BANDGAP_FILTER_BYPASS: Field = Field::new(1, 6, udma_adc::REG_CR_ADC);
    /// Bits[12:8]: ADC clock cycles per conversion (must be ≥ 14)
    pub const DATA_COUNT: Field = Field::new(5, 8, udma_adc::REG_CR_ADC);
    /// Bit[13]: Bandgap / temperature sensor enable
    pub const SENSOR_EN: Field = Field::new(1, 13, udma_adc::REG_CR_ADC);
    /// Bit[14]: ADC enable
    pub const ADC_EN: Field = Field::new(1, 14, udma_adc::REG_CR_ADC);
    /// Bit[15]: ADC reset (write 1 to reset)
    pub const ADC_RST: Field = Field::new(1, 15, udma_adc::REG_CR_ADC);
    /// Bits[23:16]: Clock frequency divider.
    /// adc_clk = perclk / (2 × FD). The resulting adc_clk must be 0.2–1.6 MHz.
    pub const CLK_FD: Field = Field::new(8, 16, udma_adc::REG_CR_ADC);
    /// Bits[25:24]: ADC input mux
    ///   00 = temperature-related voltage (internal)
    ///   01 = external voltage
    pub const ADC_SEL: Field = Field::new(2, 24, udma_adc::REG_CR_ADC);
    /// Bits[27:26]: External analog input select
    ///   00 = PA04 (ADC0),  01 = PA05 (ADC1),
    ///   10 = PA06 (ADC2),  11 = PA07 (ADC3)
    pub const VIN_SEL: Field = Field::new(2, 26, udma_adc::REG_CR_ADC);
}

// ============================================================================
// Public types
// ============================================================================

/// External analog input channel (PA04–PA07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum AdcExtChannel {
    /// PA04 / ADC0
    Adc0 = 0,
    /// PA05 / ADC1
    Adc1 = 1,
    /// PA06 / ADC2
    Adc2 = 2,
    /// PA07 / ADC3
    Adc3 = 3,
}

impl From<AdcExtChannel> for usize {
    fn from(ch: AdcExtChannel) -> usize { ch as usize }
}

impl From<usize> for AdcExtChannel {
    fn from(val: usize) -> AdcExtChannel {
        match val {
            0 => AdcExtChannel::Adc0,
            1 => AdcExtChannel::Adc1,
            2 => AdcExtChannel::Adc2,
            3 => AdcExtChannel::Adc3,
            _ => unimplemented!("AdcExtChannel value out of range: {}", val),
        }
    }
}

/// ADC measurement source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcSource {
    /// Internal temperature sensor
    Temperature,
    /// External voltage on the specified channel
    Ext(AdcExtChannel),
}

impl AdcSource {
    pub fn to_usize(self) -> usize {
        match self {
            AdcSource::Temperature => 0,
            AdcSource::Ext(ch) => 1 + ch as usize, // 1..=4
        }
    }

    pub fn from_usize(val: usize) -> AdcSource {
        match val {
            0 => AdcSource::Temperature,
            1 => AdcSource::Ext(AdcExtChannel::Adc0),
            2 => AdcSource::Ext(AdcExtChannel::Adc1),
            3 => AdcSource::Ext(AdcExtChannel::Adc2),
            4 => AdcSource::Ext(AdcExtChannel::Adc3),
            _ => unimplemented!("AdcSource::from_usize: invalid value {}", val),
        }
    }
}

/// Target ADC conversion clock in kHz.  The divider is computed from `perclk`
/// so that adc_clk lands as close to this as possible within the valid
/// 200–1600 kHz range.
const ADC_TARGET_CLK_KHZ: u32 = 1_000;

/// Minimum clock cycles per conversion (from the datasheet).
const ADC_MIN_DATA_COUNT: u32 = 14;

/// The ADC is RX-only; the entire IFRAM page is the receive buffer.
pub const ADC_RX_BUF_SIZE: usize = 4096;

// ============================================================================
// Driver
// ============================================================================

/// UDMA ADC block driver.
pub struct Adc {
    csr: CSR<u32>,
    /// IFRAM allocation backing the uDMA RX buffer.
    #[allow(dead_code)]
    ifram: IframRange,
    /// Current peripheral clock frequency in Hz, used to compute the ADC
    /// clock divider. Updated via [`update_perclk`].
    perclk_freq: u32,
}

/// Blanket [`Udma`] trait implementation so the generic uDMA helpers
/// (`udma_busy`, `udma_enqueue`, etc.) work on `Adc`.
impl Udma for Adc {
    fn csr_mut(&mut self) -> &mut CSR<u32> { &mut self.csr }

    fn csr(&self) -> &CSR<u32> { &self.csr }
}

impl Adc {
    // --- Construction --------------------------------------------------------

    /// Create a new ADC driver, mapping the hardware registers and allocating
    /// an IFRAM DMA buffer.
    ///
    /// `perclk_freq` is the UDMA peripheral clock frequency in Hz (derived
    /// from the system clock and the `fdper` fractional divider).
    ///
    /// # Safety
    ///
    /// Caller must ensure the global UDMA state has been initialized (clocks
    /// un-gated, events configured) before calling this, and must manually
    /// gate the ADC clock again when the driver is no longer needed.
    #[cfg(feature = "std")]
    pub unsafe fn new(perclk_freq: u32) -> Self {
        let adc_mem = xous::syscall::map_memory(
            xous::MemoryAddress::new(udma_adc::HW_UDMA_ADC_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map ADC registers");

        let csr = CSR::new(adc_mem.as_mut_ptr() as *mut u32);
        let ifram = IframRange::request(ADC_RX_BUF_SIZE, None).expect("couldn't allocate IFRAM for ADC");

        let mut adc = Adc { csr, ifram, perclk_freq };
        adc.init();
        adc
    }

    /// Create a bare-metal ADC driver (no MMU, physical addresses used
    /// directly).
    ///
    /// # Safety
    ///
    /// Same preconditions as [`new`], plus the caller is responsible for
    /// ensuring `udma_buf_phys` points to a valid, word-aligned DMA buffer
    /// region of at least [`ADC_RX_BUF_SIZE`] bytes that is accessible to the
    /// uDMA engine.
    #[cfg(not(feature = "std"))]
    pub unsafe fn new_baremetal(perclk_freq: u32, udma_buf_phys: usize) -> Self {
        let csr = CSR::new(udma_adc::HW_UDMA_ADC_BASE as *mut u32);
        let ifram = IframRange::from_raw_parts(
            udma_buf_phys,
            udma_buf_phys, // virt == phys in bare-metal
            ADC_RX_BUF_SIZE,
        );

        let mut adc = Adc { csr, ifram, perclk_freq };
        adc.init();
        adc
    }

    /// Re-acquire a handle to a previously initialized ADC.
    ///
    /// This is the equivalent of the UART's `get_handle` — use it when the
    /// hardware was already configured (e.g. by the loader) and you just need
    /// a driver object to operate on it.
    ///
    /// # Safety
    ///
    /// Only safe when the ADC hardware is already initialized and the
    /// provided addresses are valid.
    pub unsafe fn get_handle(
        csr_virt_addr: usize,
        udma_phys_addr: usize,
        udma_virt_addr: usize,
        perclk_freq: u32,
    ) -> Self {
        let csr = CSR::new(csr_virt_addr as *mut u32);
        let ifram = IframRange::from_raw_parts(udma_phys_addr, udma_virt_addr, ADC_RX_BUF_SIZE);
        Adc { csr, ifram, perclk_freq }
    }

    // --- Clock management ---------------------------------------------------

    /// Compute the clock frequency divider for a given peripheral clock.
    ///
    /// `perclk_khz`: peripheral clock in kHz.
    /// `target_khz`: desired ADC clock in kHz (clamped to 200–1600).
    ///
    /// The hardware divides: adc_clk = perclk / (2 × FD).
    fn calc_clk_fd(perclk_khz: u32, target_khz: u32) -> u32 {
        let target_khz = target_khz.clamp(200, 1600);
        let fd = perclk_khz / (2 * target_khz);
        fd.clamp(1, 0xFF)
    }

    /// Notify the driver that the peripheral clock frequency has changed
    /// (e.g. after a power-mode transition).  Re-computes and writes the
    /// ADC clock divider.
    ///
    /// `perclk_freq`: new peripheral clock in **Hz**.
    pub fn update_perclk(&mut self, perclk_freq: u32) {
        self.perclk_freq = perclk_freq;

        let fd = Self::calc_clk_fd(perclk_freq / 1_000, ADC_TARGET_CLK_KHZ);

        // Read-modify-write: touch only CLK_FD[23:16]
        let mut cr = self.csr.r(udma_adc::REG_CR_ADC);
        cr &= !((cr_adc::CLK_FD.mask() as u32) << cr_adc::CLK_FD.offset());
        cr |= fd << cr_adc::CLK_FD.offset();
        self.csr.wo(udma_adc::REG_CR_ADC, cr);
    }

    // --- Delays -------------------------------------------------------------

    #[cfg(feature = "std")]
    fn delay_us(us: u64) {
        if us < 1000 {
            // less than 1ms is not reliable in std
            std::thread::sleep(std::time::Duration::from_millis(1));
        } else {
            std::thread::sleep(std::time::Duration::from_micros(us));
        }
    }

    /// Bare-metal delay stub.
    #[cfg(not(feature = "std"))]
    fn delay_us(us: u64) {
        // abuse the d11ctime timer to create some time-out like thing
        let mut d11c = CSR::new(utra::d11ctime::HW_D11CTIME_BASE as *mut u32);
        d11c.wfo(utra::d11ctime::CONTROL_COUNT, 12000); // empirically tested; if values seem too low adjust higher
        let mut polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
        for _ in 0..us {
            while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
            polarity = d11c.rf(utra::d11ctime::HEARTBEAT_BEAT);
        }
        // we have to split this because we don't know where we caught the previous interval
        if us == 1 {
            while polarity == d11c.rf(utra::d11ctime::HEARTBEAT_BEAT) {}
        }
    }

    // --- Initialization -----------------------------------------------------

    /// Full ADC power-on sequence per the datasheet start timing spec:
    ///
    ///  1. Stop any in-flight DMA
    ///  2. Assert reset, set clock divider + data count + sensor enable
    ///  3. Wait ≥ 10 µs  (t1)
    ///  4. Enable bandgap reference + voltage buffers
    ///  5. Wait           (t2 — spec says 0 µs min, we add margin)
    ///  6. Enable ADC
    ///  7. Wait ≥ 90 µs  (t3)
    ///
    /// Defaults to temperature sensor source after init.
    fn init(&mut self) {
        // Step 0 — stop any prior DMA
        let rx_cfg = self.csr.r(udma_adc::REG_RX_CFG);
        if (rx_cfg & ((1 << 4) | (1 << 5))) != 0 {
            self.csr.rmwf(udma_adc::REG_RX_CFG_R_RX_CLR, 1);
        }

        // Step 1 — reset + base config
        self.csr.wo(udma_adc::REG_CR_ADC, 0);

        let fd = Self::calc_clk_fd(self.perclk_freq / 1_000, ADC_TARGET_CLK_KHZ);

        let mut cr: u32 = 0;
        cr |= 1 << cr_adc::ADC_RST.offset();
        cr |= fd << cr_adc::CLK_FD.offset();
        cr |= ADC_MIN_DATA_COUNT << cr_adc::DATA_COUNT.offset();
        cr |= 1 << cr_adc::SENSOR_EN.offset();
        self.csr.wo(udma_adc::REG_CR_ADC, cr);

        // Step 2 — t1
        Self::delay_us(20);

        // Step 3 — bandgap + temperature buffers
        cr |= 1 << cr_adc::BANDGAP_BUF_EN.offset();
        cr |= 1 << cr_adc::TEMP_BUF_EN.offset();
        cr |= 1 << cr_adc::TEMP_FILTER_BYPASS.offset();
        cr |= 1 << cr_adc::BANDGAP_FILTER_BYPASS.offset();
        self.csr.wo(udma_adc::REG_CR_ADC, cr);

        // Step 4 — t2
        Self::delay_us(20);

        // Step 5 — enable ADC
        cr |= 1 << cr_adc::ADC_EN.offset();
        self.csr.wo(udma_adc::REG_CR_ADC, cr);

        // Step 6 — t3
        Self::delay_us(120);
    }

    // --- Source selection ----------------------------------------------------

    /// Reconfigure the analog mux for the given source.
    ///
    /// Read-modify-writes only the buffer-enable and mux-select bits.
    pub fn configure_source(&mut self, source: AdcSource) {
        let mut cr = self.csr.r(udma_adc::REG_CR_ADC);

        // Clear mux-related bits in one shot
        cr &= !((1u32 << cr_adc::TEMP_BUF_EN.offset())
            | (1u32 << cr_adc::EXT_BUF_EN.offset())
            | ((cr_adc::ADC_SEL.mask() as u32) << cr_adc::ADC_SEL.offset())
            | ((cr_adc::VIN_SEL.mask() as u32) << cr_adc::VIN_SEL.offset()));

        match source {
            AdcSource::Temperature => {
                cr |= 1 << cr_adc::TEMP_BUF_EN.offset();
                // ADC_SEL = 00 (temperature), VIN_SEL = don't-care
            }
            AdcSource::Ext(ch) => {
                cr |= 1 << cr_adc::EXT_BUF_EN.offset();
                cr |= 1 << cr_adc::ADC_SEL.offset(); // 01 = external
                cr |= (ch as u32) << cr_adc::VIN_SEL.offset();
            }
        }

        self.csr.wo(udma_adc::REG_CR_ADC, cr);
        Self::delay_us(20);
    }

    // --- Measurement --------------------------------------------------------

    /// Blocking single-shot read.  Returns the raw 10-bit result (0–1023).
    pub fn read_raw(&mut self, source: AdcSource) -> u16 {
        self.configure_source(source);

        self.csr.rmwf(udma_adc::REG_RX_CFG_R_RX_CLR, 1);

        // Enqueue a 1-word (4-byte) DMA transfer
        #[cfg(feature = "std")]
        unsafe {
            self.udma_enqueue(Bank::Rx, &self.ifram.as_phys_slice::<u8>()[..4], CFG_EN | CFG_SIZE_32);
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            self.udma_enqueue(Bank::Rx, &self.ifram.as_phys_slice::<u8>()[..4], CFG_EN | CFG_SIZE_32);
        }

        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }

        #[cfg(any(feature = "std", feature = "kernel"))]
        let val = self.ifram.as_slice::<u32>()[0];
        #[cfg(not(any(feature = "std", feature = "kernel")))]
        let val = unsafe { self.ifram.as_phys_slice::<u32>()[0] };

        (val & 0x3FF) as u16
    }

    /// Read `n` samples and return the average (raw 10-bit value).
    pub fn read_raw_averaged(&mut self, source: AdcSource, n: usize) -> u16 {
        let max_words = ADC_RX_BUF_SIZE / core::mem::size_of::<u32>();
        assert!(n > 0 && n <= max_words, "n must be 1..={}", max_words);

        self.configure_source(source);

        self.csr.rmwf(udma_adc::REG_RX_CFG_R_RX_CLR, 1);

        let byte_len = n * core::mem::size_of::<u32>();
        #[cfg(feature = "std")]
        unsafe {
            self.udma_enqueue(Bank::Rx, &self.ifram.as_phys_slice::<u8>()[..byte_len], CFG_EN | CFG_SIZE_32);
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            self.udma_enqueue(Bank::Rx, &self.ifram.as_phys_slice::<u8>()[..byte_len], CFG_EN | CFG_SIZE_32);
        }

        while self.udma_busy(Bank::Rx) {
            #[cfg(feature = "std")]
            xous::yield_slice();
        }

        let mut sum: u32 = 0;
        for i in 0..n {
            #[cfg(any(feature = "std", feature = "kernel"))]
            let val = self.ifram.as_slice::<u32>()[i];
            #[cfg(not(any(feature = "std", feature = "kernel")))]
            let val = unsafe { self.ifram.as_phys_slice::<u32>()[i] };

            sum += val & 0x3FF;
        }
        (sum / n as u32) as u16
    }

    /// Arm continuous DMA conversion.  The uDMA will repeatedly fill the
    /// buffer and wrap.  Use the ADC RX interrupt (IRQn 121 - irqarray8::EV_PENDING_ADC_RX) or poll
    /// `udma_busy(Bank::Rx)` to track progress.
    pub fn start_continuous(&mut self, source: AdcSource) {
        self.configure_source(source);

        self.csr.rmwf(udma_adc::REG_RX_CFG_R_RX_CLR, 1);

        #[cfg(feature = "std")]
        unsafe {
            self.udma_enqueue(Bank::Rx, self.ifram.as_phys_slice::<u8>(), CFG_EN | CFG_SIZE_32 | CFG_CONT);
        }
        #[cfg(not(feature = "std"))]
        unsafe {
            self.udma_enqueue(Bank::Rx, self.ifram.as_phys_slice::<u8>(), CFG_EN | CFG_SIZE_32 | CFG_CONT);
        }
    }

    /// Stop continuous conversion.
    pub fn stop_continuous(&mut self) { self.csr.rmwf(udma_adc::REG_RX_CFG_R_RX_CLR, 1); }

    // --- Conversion helpers -------------------------------------------------

    /// Raw → temperature in °C (float).
    ///
    /// Linear regression on the datasheet table
    ///   T = −0.5877 × raw + 328.68,  R² ≈ 0.99997
    pub fn raw_to_temp_celsius(raw: u16) -> f32 { -0.587_744_8 * raw as f32 + 328.679_84 }

    /// Raw → temperature in tenths of °C (integer).
    /// e.g. 253 means 25.3 °C.
    pub fn raw_to_temp_tenths_c(raw: u16) -> i16 { 3287i16 - (raw as i32 * 587_745 / 100_000) as i16 }

    /// Raw → voltage in volts (bandgap ref, Vbg = 1.208 V).
    ///   V_in = raw / 1023 × 1.208
    pub fn raw_to_voltage(raw: u16) -> f32 { (raw as f32 / 1023.0) * 1.208 }
}
