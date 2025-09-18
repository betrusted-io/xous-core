use bitflags::*;
use utralib::generated::*;

const START_CODE: u32 = 0x5A;
const STOP_CODE: u32 = 0xA5;

bitflags! {
    pub struct EntropySource: u32 {
        const LOW_FREQ_EN        = 0b0000000_000_01;
        const HIGH_FREQ_EN       = 0b0000000_000_10;
        const LOW_FREQ_SRC_MASK  = 0b0000000_111_00;
        const HIGH_FREQ_SRC_MASK = 0b1111111_000_00;
    }
}

bitflags! {
    pub struct Analog: u32 {
        const VALID_MASK  = 0b00000000_11111111;
        const ENABLE_MASK = 0b11111111_00000000;
    }
}

bitflags! {
    pub struct Options: u32 {
        const GENERATION_COUNT_POS  = 0x0;
        const GENERATION_COUNT_MASK = 0x0_FFFF;
        const SEGMENT_A_SELECT      = 0x0_0000;
        const SEGMENT_B_SELECT      = 0x1_0000;
        const SEGMENT_SEL_MASK      = 0x1_0000;
    }
}

bitflags! {
    pub struct Config: u32 {
        const GEN_EN               = 0b0_00_00_000000_00_0_0_0_1;
        const PARITY_FILTER_EN     = 0b0_00_00_000000_00_0_0_1_0;
        const HEALTHEST_EN         = 0b0_00_00_000000_00_0_1_0_0;
        const DRNG_EN              = 0b0_00_00_000000_00_1_0_0_0;
        const POSTPROC_OPT_MASK    = 0b0_00_00_000000_11_0_0_0_0;

        const POSTPROC_OPT_LFSR    = 0b0_00_00_000000_00_0_0_0_0;
        const POSTPROC_OPT_AES     = 0b0_00_00_000000_01_0_0_0_0;
        const POSTPROC_OPT_RESEED_ALWAYS  = 0b0_00_00_000000_10_0_0_0_0;
        const POSTPROC_OPT_RESEED_AUTO    = 0b0_00_00_000000_10_0_0_0_0;

        const HEALTHTEST_LEN_POS   = 6;
        const HEALTHTEST_LEN_MASK  = 0b0_00_00_111111_00_0_0_0_0;
        const GEN_INTERVAL_MASK    = 0b0_00_11_000000_00_0_0_0_0;
        const GEN_INTERVAL_1       = 0b0_00_00_000000_00_0_0_0_0;
        const GEN_INTERVAL_2       = 0b0_00_01_000000_00_0_0_0_0;
        const GEN_INTERVAL_4       = 0b0_00_10_000000_00_0_0_0_0;
        const GEN_INTERVAL_8       = 0b0_00_11_000000_00_0_0_0_0;
        const RESEED_INTERVAL_MASK = 0b0_11_00_000000_00_0_0_0_0;
        const RESEED_INTERVAL_NEVER= 0b0_00_00_000000_00_0_0_0_0;
        const RESEED_INTERVAL_1    = 0b0_01_00_000000_00_0_0_0_0;
        const RESEED_INTERVAL_128  = 0b0_10_00_000000_00_0_0_0_0;
        const RESEED_INTERVAL_1024 = 0b0_11_00_000000_00_0_0_0_0;
        const RESEED_SEL           = 0b1_00_00_000000_00_0_0_0_0;
    }
}

bitflags! {
    pub struct Status: u32 {
        const GEN_COUNT_MASK          = 0b0_0__0000_0000_0000__1111_1111_1111_1111;
        const HEALTHTEST_ERRCNT_MASK  = 0b0_0__1111_1111_1111__0000_0000_0000_0000;
        const BUFREADY                = 0b0_1__0000_0000_0000__0000_0000_0000_0000;
        const DRNG_REESED_REQ         = 0b1_0__0000_0000_0000__0000_0000_0000_0000;
    }
}

#[derive(PartialEq, Eq)]
pub enum Mode {
    Uninit,
    Raw,
    /// TODO
    _Lfsr,
    /// TODO
    _Aes,
}

const RAW_ENTRIES: usize = 16;
/// The guardband is a number of entries of the TRNG to dispose of after
/// sampling for QC. The idea is to allow the TRNG internal state to evolve
/// for at least this many cycles before the next sample is taken, thus
/// making it more difficult for any adversary to reason about the current
/// state of the TRNG given the QC samples.
#[allow(dead_code)]
const RAW_GUARDBAND: usize = 2;

pub struct Trng {
    pub csr: CSR<u32>,
    _count: u16, // vestigial, to be removed?
    mode: Mode,
    /// Buffer some raw entropy inside the kernel, so we can "burst out" entropy
    /// for reseed operations without having to wait for the TRNG to regenerate data.
    ///
    /// This generates an unused warning when `verilator-only` is selected. We leave this
    /// warning in place to remind ourselves that the TRNG is not real.
    raw: [Option<u32>; RAW_ENTRIES],
    #[cfg(feature = "compress-entropy")]
    rng_var: u8,
}

impl Trng {
    pub fn new(base_addr: usize) -> Self {
        let csr = CSR::new(base_addr as *mut u32);
        #[cfg(feature = "compress-entropy")]
        {
            Trng { csr, _count: 0, mode: Mode::Uninit, raw: [None; RAW_ENTRIES], rng_var: 0 }
        }
        #[cfg(not(feature = "compress-entropy"))]
        {
            Trng { csr, _count: 0, mode: Mode::Uninit, raw: [None; RAW_ENTRIES] }
        }
    }

    #[cfg(not(feature = "verilator-only"))]
    pub fn setup_raw_generation(&mut self, count: u16) {
        self._count = count;
        self.mode = Mode::Raw;

        self.csr.wo(
            utra::trng::SFR_CRSRC,
            (EntropySource::LOW_FREQ_EN | EntropySource::HIGH_FREQ_EN | EntropySource::LOW_FREQ_SRC_MASK)
                .bits(),
        );
        // turn on all the analog generators, and declare their outputs valid
        self.csr.wo(utra::trng::SFR_CRANA, (Analog::ENABLE_MASK | Analog::VALID_MASK).bits());

        self.csr.wo(utra::trng::SFR_PP, (Config::PARITY_FILTER_EN).bits());

        self.csr.wo(utra::trng::SFR_OPT, 0x1_0000); // TrngB

        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN0, 0xFFFFFFFF);
        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN1, 0xFFFFFFFF);
        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN2, 0xFFFFFFFF);
        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN3, 0xFFFFFFFF);
    }

    #[cfg(feature = "verilator-only")]
    pub fn setup_raw_generation(&mut self, _count: u16) { self.mode = Mode::Raw; }

    /// Function used primarily for re-entrant testing. Forces the mode to a given
    /// mode. Only safe if the TRNG is actually already in that mode!
    pub unsafe fn force_mode(&mut self, mode: Mode) { self.mode = mode; }

    #[cfg(not(feature = "verilator-only"))]
    pub fn get_raw(&mut self) -> u32 {
        // Pull from the buffered entropy pool, until it's empty.
        for d in self.raw.iter_mut() {
            if let Some(r) = d.take() {
                return r;
            }
        }

        // If empty, refill the buffer.
        for d in self.raw.iter_mut() {
            // With the adjusted settings for the TRNG, the output is not perfect, but
            // substantially better than before. Previously it was maybe one bit per
            // 64 bits in entropy, now it looks like better than 0.5. Still more analysis
            // needs to be done, there are some subtle biases in the generator but they
            // are small enough we can pass the numbers directly into the CSPRNG for
            // mixing without compression.
            while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
            *d = Some(self.csr.r(utra::trng::SFR_BUF));
        }

        // Run the TRNG state forward for some number of cycles to make it harder to draw
        // any conclusions about the TRNG's state based on the reported raw samples.
        for _ in 0..RAW_GUARDBAND {
            while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
            let _ = Some(self.csr.r(utra::trng::SFR_SR));
        }

        // return the first element of the generated array
        self.raw[0].take().unwrap()
    }

    #[cfg(feature = "verilator-only")]
    pub fn get_raw(&mut self) -> u32 {
        // guaranteed random by fair dice roll
        // (in all seriousness, this is intentional in verilator because there is no TRNG in the RTL model)
        4
    }

    pub fn get_u32(&mut self) -> Option<u32> {
        match self.mode {
            Mode::Uninit => None,
            Mode::Raw => Some(self.get_raw()),
            Mode::_Lfsr => {
                todo!("LFSR mode not yet implemented");
            }
            Mode::_Aes => {
                todo!("AES mode not yet implemented");
            }
        }
    }

    pub fn get_raw_count(&self) -> u16 {
        (self.csr.r(utra::trng::SFR_SR) & Status::GEN_COUNT_MASK.bits()) as u16
    }

    pub fn get_count_remaining(&self) -> u16 { self._count }

    pub fn start(&mut self) { self.csr.wo(utra::trng::SFR_AR_GEN, START_CODE); }

    pub fn stop(&mut self) { self.csr.wo(utra::trng::SFR_AR_GEN, STOP_CODE); }
}
