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
        const GEN_COUNT_MASK          = 0b0_0__0000_0000__1111_1111_1111_1111;
        const HEALTHTEST_ERRCNT_MASK  = 0b0_0__1111_1111__0000_0000_0000_0000;
        const BUFREADY                = 0b0_1__0000_0000__0000_0000_0000_0000;
        const DRNG_REESED_REQ         = 0b1_0__0000_0000__0000_0000_0000_0000;
    }
}

#[derive(PartialEq, Eq)]
enum Mode {
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
const RAW_GUARDBAND: usize = 16;

pub struct Trng {
    pub csr: CSR<u32>,
    _count: u16, // vestigial, to be removed?
    mode: Mode,
    raw: [Option<u32>; RAW_ENTRIES],
}

impl Trng {
    pub fn new(base_addr: usize) -> Self {
        let csr = CSR::new(base_addr as *mut u32);
        Trng { csr, _count: 0, mode: Mode::Uninit, raw: [None; RAW_ENTRIES] }
    }

    pub fn setup_raw_generation(&mut self, count: u16) {
        self._count = count;
        self.mode = Mode::Raw;
        // turn on all the entropy sources
        self.csr.wo(
            utra::trng::SFR_CRSRC,
            (EntropySource::LOW_FREQ_EN
                | EntropySource::HIGH_FREQ_EN
                | EntropySource::LOW_FREQ_SRC_MASK
                | EntropySource::HIGH_FREQ_SRC_MASK)
                .bits(),
        );
        // turn on all the analog generators, and declare their outputs valid
        self.csr.wo(utra::trng::SFR_CRANA, (Analog::ENABLE_MASK | Analog::VALID_MASK).bits());
        // enable the rng chains
        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN0, 0xffff_ffff);
        self.csr.wo(utra::trng::SFR_CHAIN_RNGCHAINEN1, 0xffff_ffff);

        /*
        // set options
        self.csr.wo(
            utra::trng::SFR_OPT,
            (((count as u32) << Options::GENERATION_COUNT_POS.bits())
                & Options::GENERATION_COUNT_MASK.bits())
                | Options::SEGMENT_B_SELECT.bits(),
        );
        // set configuration options
        let healthest_len =
            if count as u32 > (Config::HEALTHTEST_LEN_MASK.bits() >> Config::HEALTHTEST_LEN_POS.bits()) {
                Config::HEALTHTEST_LEN_MASK.bits() >> Config::HEALTHTEST_LEN_POS.bits()
            } else {
                count as u32
            };
        self.csr.wo(
            utra::trng::SFR_PP,
            (Config::GEN_EN | Config::GEN_INTERVAL_4 | Config::RESEED_INTERVAL_1).bits()
                | (healthest_len << Config::HEALTHTEST_LEN_POS.bits()) & Config::HEALTHTEST_LEN_MASK.bits(),
        );
        */
        self.csr.wo(
            utra::trng::SFR_PP,
            (Config::GEN_EN | Config::GEN_INTERVAL_4 | Config::RESEED_INTERVAL_1).bits()
                | Config::HEALTHEST_EN.bits(),
        );
        self.csr.wo(utra::trng::SFR_OPT, 0);
    }

    pub fn get_raw(&mut self) -> u32 {
        for d in self.raw.iter_mut() {
            if let Some(r) = d.take() {
                return r;
            }
        }
        while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
        for d in self.raw.iter_mut() {
            *d = Some(self.csr.r(utra::trng::SFR_BUF));
        }
        let r = self.csr.r(utra::trng::SFR_BUF);
        while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
        // Run the TRNG state forward for some number of cycles to make it harder to draw
        // any conclusions about the TRNG's state based on the reported raw samples.
        for _ in 0..RAW_GUARDBAND {
            let _ = Some(self.csr.r(utra::trng::SFR_SR));
        }
        r
    }

    pub fn get_u32(&mut self) -> Option<u32> {
        match self.mode {
            Mode::Uninit => None,
            Mode::Raw => {
                /*
                if self.count > 0 {
                    self.count -= 1;
                    while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
                    Some(self.csr.r(utra::trng::SFR_BUF))
                } else {
                    // re-init generation automatically
                    self.setup_raw_generation(32);
                    self.count -= 1;
                    while self.csr.r(utra::trng::SFR_SR) & Status::BUFREADY.bits() == 0 {}
                    Some(self.csr.r(utra::trng::SFR_BUF))
                }
                */
                Some(self.get_raw())
            }
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

// some old test code
#[cfg(feature = "delete-me-when-done")]
pub fn trng_test() {
    let mut trng = sce::trng::Trng::new(HW_TRNG_BASE);
    trng.setup_raw_generation(256);
    for _ in 0..8 {
        crate::println!("trng raw: {:x}", trng.get_u32().unwrap_or(0xDEAD_BEEF));
    }
    let trng_csr = CSR::new(HW_TRNG_BASE as *mut u32);
    /*
    let mut trng = CSR::new(HW_TRNG_BASE as *mut u32);
    trng.wo(utra::trng::SFR_CRSRC, 0xFFFF);
    trng.wo(utra::trng::SFR_CRANA, 0xFFFF);
    trng.wo(utra::trng::SFR_OPT, 0x10020);
    trng.wo(utra::trng::SFR_PP, 0x1 << 14 | 0x2 << 12 | 0x20 << 6 | 1);

    while (trng.r(utra::trng::SFR_SR) & (1 << 24)) == 0 {}
    for _ in 0..8 {
        crate::println!("trng raw: {:x}", trng.r(utra::trng::SFR_BUF));
    }
    */
    // used for auto DMA i think...
    #[cfg(feature = "autodma")]
    {
        trng.wo(utra::trng::SFR_AR_GEN, 0x5A); // start
        while trng.rf(utra::trng::SFR_FR_SFR_FR) == 0 {}
        trng.wfo(utra::trng::SFR_FR_SFR_FR, 1);
        trng.wo(utra::trng::SFR_AR_GEN, 0xA5); // stop
        let seg_rngb = unsafe {
            core::slice::from_raw_parts(utralib::HW_SEG_RNGB_MEM as *const u32, utralib::HW_SEG_RNGB_MEM_LEN)
        };
        for &w in seg_rngb[..16].iter() {
            crate::println!("trng: {:x}", w)
        }
    }
}
