use bao1x_api::find_pll_params;
#[cfg(all(feature = "std", feature = "board-baosec"))]
use bao1x_api::{IoxHal, IoxPort};
use bitbybit::bitfield;
use utralib::*;

#[cfg(feature = "board-baosec")]
use crate::axp2101::Axp2101;
#[cfg(all(feature = "std", feature = "board-baosec"))]
use crate::i2c::I2c;

#[cfg(feature = "std")]
const MHZ: u32 = 1_000_000;

/// This is always the default target rate for peripheral clock, regardless of the pll setting
/// It's not always achievable - for example, when the PLL is turned off, which is why the actual
/// perclk is returned by the clock setting routines.
pub const PERCLK_HZ: u32 = 100_000_000;

pub const HCLK_HZ: u32 = 200_000_000;
pub const ICLK_HZ: u32 = 100_000_000;
pub const PCLK_HZ: u32 = 50_000_000;

#[bitfield(u32)]
#[derive(PartialEq, Eq, Debug)]
pub struct PmuControl {
    #[bit(6, rw)]
    pmu_2p5v_ena: bool,
    #[bit(5, rw)]
    pmu_0p8v_ana_ena: bool,
    #[bit(4, rw)]
    pmu_0p8v_dig_ena: bool,
    #[bit(3, rw)]
    iout_ref_ena: bool,
    #[bit(2, rw)]
    power_on_control: bool,
    #[bit(1, rw)]
    // when true sets 0.8v regulator to 0.9v
    pmu_0p8v_ana_hv_ena: bool,
    #[bit(0, rw)]
    // when true sets 0.8v regulator to 0.9v
    pmu_0p8v_dig_hv_ena: bool,
}

/// Requires:
/// `freq_hz`: target frequency of PLL - generally it is 2x of the CPU clock frequency
/// `daric_cgu`: Pointer to SYSCTL_BASE - for setting clocks
/// `ao_sysctl`: Pointer to AO_SYSCTRL_BASE - for setting regulators according to clocks
/// `duart`: Pointer to DUART_BASE - for setting the DUART's ETUC divider - only set if provided
/// `delay_at_sysfreq(usize, u32)`: Function that can perform a delay of usize milliseconds at u32 hz
/// this is needed to wait while the power regulators adjust.
/// `fast_bio`: when `true`, BIO clocks at 2x CPU clock. This increases baseline power consumption
/// by about 12mW. Not a big deal if the board is plugged in, but significant for battery-powered systems.
///
/// (deprecated) `udma_ctrl_base`: Pointer to HW_UDMA_CTRL_BASE - for clearing the UDMA clock gates
///
/// Register offsets are passed in so the routine can also be used in `std` environments, by a manager
/// that already has mapping to the required registers. `duart` is not always available in the manager's
/// space, which is why it's an Option<usize>.
///
/// The coding style is a little awkward in this routine - a number of registers are unpacked
/// using direct offsets and a write_volatile(). This was done to ensure that the compiler is not
/// optimizing out repeated writes. This could be cleaned up to use the UTRA abstractions, but because
/// the code works and it was pretty hard to get it validated, there is an incentive not to touch it.
pub unsafe fn init_clock_asic<F>(
    freq_hz: u32,
    cgu_base: usize,
    ao_sysctl_base: usize,
    duart_base: Option<usize>,
    delay_at_sysfreq: F,
    fast_bio: bool,
) -> u32
where
    F: Fn(usize, u32),
{
    use utra::sysctrl;
    let daric_cgu = cgu_base as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    // compute the params first. Consider passing errors up the stack but for now panic.
    let pll_params = find_pll_params(freq_hz, true).expect("Couldn't find valid PLL solution");

    // Safest divider settings, assuming no overclocking.
    // If overclocking, need to lower hclk:iclk:pclk even futher; the CPU speed can outperform the bus fabric.
    // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
    // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
    if fast_bio {
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x0700_01ff); // fclk
    } else {
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x0700_017f); // fclk
    }
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x0f00_0f7f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f00_073f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x3f00_031f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x7f00_010f); // pclk

    // peripheral clock should always target 100MHz regardless of the top frequency
    let peri_params = bao1x_api::find_optimal_divider(freq_hz / 2, PERCLK_HZ).unwrap();
    let fd0 = peri_params.fd0;
    let fd2 = peri_params.fd2;
    daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(
        (peri_params.fd0 as u32) << 16 | (peri_params.fd2 as u32) << 8 | peri_params.fd2 as u32,
    );
    let perclk = peri_params.actual_freq_hz;

    // configure gates
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x00); // mbox/qfc turned off
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x02); // mdma off, sce on
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x90); // bio/udc enable
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x80); // enable mesh
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

    // 0: RC, 1: XTAL
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, bao1x_api::FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    // update the DUART to use XTAL params
    if let Some(duart_ptr) = duart_base {
        let duart = duart_ptr as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        // set the ETUC now that we're on the xosc.
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(bao1x_api::FREQ_OSC_MHZ);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }

    // switch to OSC - now clocking off of external oscillator (glitch hazard!)
    // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    // set internal LDO to match the requested frequency
    let mut ao_sysctrl = CSR::new(ao_sysctl_base as *mut u32);
    if freq_hz > 700_000_000 {
        // crate::println!("setting vdd85 to 0.893v");
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421FF1);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        delay_at_sysfreq(20, 48_000_000);
    } else if freq_hz > 350_000_000 {
        // crate::println!("setting vdd85 to 0.81v");
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421290);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        delay_at_sysfreq(20, 48_000_000);
    } else {
        // crate::println!("setting vdd85 to 0.72v");
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420420);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        delay_at_sysfreq(20, 48_000_000);
    }

    // PD PLL
    cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    for _ in 0..4096 {
        bao1x_api::bollard!(4);
    }

    cgu.wo(
        sysctrl::SFR_IPCPLLMN,
        (((pll_params.m as u32) << 12) & 0x0001F000) | ((pll_params.n as u32) & 0x00000fff),
    );
    if pll_params.frac == 0 {
        cgu.wo(sysctrl::SFR_IPCPLLF, 0);
    } else {
        cgu.wo(sysctrl::SFR_IPCPLLF, pll_params.frac | (1 << 24));
    }
    // TODO: don't cheeseball the pll1 output dividers to be 1/4th of pll0 - use a PKE target frequency
    // instead
    cgu.wo(
        sysctrl::SFR_IPCPLLQ,
        (4 * (pll_params.q1 as u32 - 1) << 12)
            | 4 * (pll_params.q0 as u32 - 1) << 8
            | ((pll_params.q1 as u32 - 1) << 4)
            | pll_params.q0 as u32 - 1,
    );

    //               VCO bias   CPP bias   CPI bias
    //                1          2          3
    cgu.wo(sysctrl::SFR_IPCCR, (1 << 6) | (2 << 3) | (3));
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);

    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    for _ in 0..4096 {
        bao1x_api::bollard!(4);
    }

    bao1x_api::bollard!(6);
    cgu.wo(sysctrl::SFR_CGUSEL0, 1);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    bao1x_api::bollard!(6);

    // glitch_safety: check that we're running on the PLL
    #[cfg(all(not(feature = "kernel"), not(feature = "std")))]
    crate::hardening::check_pll();
    #[cfg(feature = "std")]
    check_pll_std(&mut cgu);

    crate::println!("mn {:x}, q{:x}", cgu.r(sysctrl::SFR_IPCPLLMN), cgu.r(sysctrl::SFR_IPCPLLQ));
    crate::println!("fsvalid: {}", daric_cgu.add(sysctrl::SFR_CGUFSVLD.offset()).read_volatile());
    let clk_desc: [(&'static str, u32, usize); 8] = [
        ("fclk", 16, 0x40 / size_of::<u32>()),
        ("pke", 0, 0x40 / size_of::<u32>()),
        ("ao", 16, 0x44 / size_of::<u32>()),
        ("aoram", 0, 0x44 / size_of::<u32>()),
        ("osc", 16, 0x48 / size_of::<u32>()),
        ("xtal", 0, 0x48 / size_of::<u32>()),
        ("pll0", 16, 0x4c / size_of::<u32>()),
        ("pll1", 0, 0x4c / size_of::<u32>()),
    ];
    for (name, shift, offset) in clk_desc {
        let fsfreq = (daric_cgu.add(offset).read_volatile() >> shift) & 0xffff;
        crate::println!("{}: {} MHz", name, fsfreq);
    }

    crate::println!(
        "Perclk solution: {:x}|{:x} -> {}.{} MHz",
        fd0,
        fd2,
        perclk / 1_000_000,
        perclk % 1_000_000
    );
    crate::println!("PLL configured to {}.{} MHz", freq_hz / 1_000_000, freq_hz % 1_000_000);

    // glitch_safety: check that we're running on the PLL
    #[cfg(all(not(feature = "kernel"), not(feature = "std")))]
    crate::hardening::check_pll();
    #[cfg(feature = "std")]
    check_pll_std(&mut cgu);

    perclk
}

#[inline(always)]
#[cfg(feature = "std")]
fn check_pll_std(cgu: &mut CSR<u32>) {
    if cgu.r(utra::sysctrl::SFR_CGUSEL0) & 1 == 0 {
        // we're not on the PLL: reboot
        cgu.wo(utra::sysctrl::SFR_RCURST0, 0x0000_55aa);
    }
}

/// maximum value representable in the fd counter
const FD_MAX: u32 = 256;

pub fn fd_from_frequency(desired_freq_hz: u32, in_freq_hz: u32) -> u32 {
    // Shift by 1_000 to prevent overflow
    let desired_freq_khz = desired_freq_hz / 1_000;
    let in_freq_khz = in_freq_hz / 1_000;

    // Calculate (fd + 1) with rounding for accuracy
    let fd_plus_1 = (desired_freq_khz * FD_MAX + in_freq_khz / 2) / in_freq_khz;

    // Subtract 1 to get fd, handling underflow and clamping to valid range
    if fd_plus_1 == 0 { 0 } else { (fd_plus_1 - 1).min(FD_MAX - 1) }
}

pub fn divide_by_fd(fd: u32, in_freq_hz: u32) -> u32 {
    // shift by 1_000 to prevent overflow
    let in_freq_khz = in_freq_hz / 1_000;
    let out_freq_khz = (in_freq_khz * (fd + 1)) / FD_MAX;
    // restore to Hz
    out_freq_khz * 1_000
}

#[cfg(feature = "std")]
pub struct ClockManager {
    pub vco_freq: u32,
    pub fclk: u32,
    pub aclk: u32,
    pub hclk: u32,
    pub iclk: u32,
    pub pclk: u32,
    pub perclk: u32,
    sysctrl: CSR<u32>,
    // this is a clone of what's in the kpc_aoint
    ao_sysctrl: CSR<u32>,
    susres: CSR<u32>,
    #[cfg(feature = "board-baosec")]
    i2c: I2c,
    #[cfg(feature = "board-baosec")]
    iox: IoxHal,
    #[cfg(feature = "board-baosec")]
    dcdc2_io: (IoxPort, u8),
    #[cfg(feature = "board-baosec")]
    pmic: Axp2101,
}

#[cfg(feature = "std")]
impl ClockManager {
    pub fn new() -> Result<Self, xous::Error> {
        let sysctrl_mem = xous::map_memory(
            xous::MemoryAddress::new(utra::sysctrl::HW_SYSCTRL_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )?;
        // this is actually double-mapped: once by us, and once by the KPC
        let ao_mem = xous::map_memory(
            xous::MemoryAddress::new(utra::ao_sysctrl::HW_AO_SYSCTRL_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )?;
        let susres = xous::map_memory(
            xous::MemoryAddress::new(utra::susres::HW_SUSRES_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )?;

        // compute the actual frequency values by reading the PLL config
        let sysctrl = CSR::new(sysctrl_mem.as_mut_ptr() as *mut u32);
        let m = (sysctrl.r(utra::sysctrl::SFR_IPCPLLMN) >> 12) & 0xF;
        let n = sysctrl.r(utra::sysctrl::SFR_IPCPLLMN) & 0xFFF;
        let q1 = (sysctrl.r(utra::sysctrl::SFR_IPCPLLQ) >> 4) & 0x7;
        let q0 = (sysctrl.r(utra::sysctrl::SFR_IPCPLLQ) >> 0) & 0x7;
        let fracen = sysctrl.r(utra::sysctrl::SFR_IPCPLLF) & 0x100_0000 != 0;
        let frac = sysctrl.r(utra::sysctrl::SFR_IPCPLLF) & 0xFF_FFFF;
        let vco_freq =
            if fracen { ((48 * n + (48 * frac) / (1 << 24)) / m) * MHZ } else { ((48 * n) / m) * MHZ };
        let pll0_freq = vco_freq / (1 + q0) / (1 + q1);
        let fclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0) & 0xFF;
        let aclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1) & 0xFF;
        let hclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2) & 0xFF;
        let iclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3) & 0xFF;
        let pclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4) & 0xFF;

        let fclk = divide_by_fd(fclk_fd, pll0_freq);
        let aclk = divide_by_fd(aclk_fd, pll0_freq);
        let hclk = divide_by_fd(hclk_fd, pll0_freq);
        let iclk = divide_by_fd(iclk_fd, pll0_freq);
        let pclk = divide_by_fd(pclk_fd, pll0_freq);
        log::info!("fracen: {:?}", fracen);
        // perclk has an extra /2 applied to it
        log::info!("perclk: {:x}, pll0_freq {}", sysctrl.r(utra::sysctrl::SFR_CGUFDPER), pll0_freq);
        let perclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFDPER) & 0xFF;
        let perclk = divide_by_fd(perclk_fd, pll0_freq) / 2;

        /*
        log::info!("m: {} n: {} q1: {} q0: {} frac: {}, fracen: {:?}", m, n, q1, q0, frac, fracen);
        if let Some(params) = bao1x_api::find_pll_params(700_000_000, true) {
            log::info!("700Mhz params: {:?}", params);
        }
        */

        #[cfg(feature = "board-baosec")]
        let iox = IoxHal::new();
        #[cfg(feature = "board-baosec")]
        let (port, pin) = crate::board::setup_dcdc2_pin(&iox);
        #[cfg(feature = "board-baosec")]
        let mut i2c = I2c::new();
        #[cfg(feature = "board-baosec")]
        let pmic = crate::axp2101::Axp2101::new(&mut i2c).unwrap();

        Ok(Self {
            vco_freq,
            fclk,
            aclk,
            hclk,
            iclk,
            pclk,
            perclk,
            sysctrl,
            ao_sysctrl: CSR::new(ao_mem.as_ptr() as *mut u32),
            susres: CSR::new(susres.as_ptr() as *mut u32),
            #[cfg(feature = "board-baosec")]
            i2c,
            #[cfg(feature = "board-baosec")]
            iox,
            #[cfg(feature = "board-baosec")]
            dcdc2_io: (port, pin),
            #[cfg(feature = "board-baosec")]
            pmic,
        })
    }

    /// Safety: this pulls out the hardware base for the susres block. The receiver of this
    /// address must manually manage all concurrency issues that could happen with respect to
    /// using the susres block.
    pub unsafe fn susres_base(&self) -> usize { self.susres.base() as usize }

    pub fn measured_freqs(&self) -> Vec<(String, u32)> {
        let mut readings = Vec::new();
        let clk_desc: [(&'static str, u32, usize); 8] = [
            ("fclk", 16, 0x40 / size_of::<u32>()),
            ("pke", 0, 0x40 / size_of::<u32>()),
            ("ao", 16, 0x44 / size_of::<u32>()),
            ("aoram", 0, 0x44 / size_of::<u32>()),
            ("osc", 16, 0x48 / size_of::<u32>()),
            ("xtal", 0, 0x48 / size_of::<u32>()),
            ("pll0", 16, 0x4c / size_of::<u32>()),
            ("pll1", 0, 0x4c / size_of::<u32>()),
        ];
        let fsvalid = self.sysctrl.r(utra::sysctrl::SFR_CGUFSVLD);
        for (i, &(name, shift, offset)) in clk_desc.iter().enumerate() {
            if (1 << i as u32) & fsvalid != 0 {
                let fsfreq = unsafe { (self.sysctrl.base().add(offset).read_volatile() >> shift) & 0xffff };
                readings.push((name.to_string(), fsfreq));
            }
        }
        readings
    }

    pub fn request_freq(&mut self, cpu_freq_mhz: u32) -> Result<u32, String> {
        if cpu_freq_mhz < 50 || cpu_freq_mhz > 800 {
            return Err("Requested frequency out of range".into());
        }
        // fclk top frequency is 2x of cpu frequency, in hz
        let fclk_freq = cpu_freq_mhz * 1_000_000 * 2;
        crate::println!("req_freq: {}", fclk_freq);
        let perclk = unsafe {
            init_clock_asic(
                fclk_freq,
                self.sysctrl.base() as usize,
                self.ao_sysctrl.base() as usize,
                None,
                |ms: usize, freq_hz: u32| {
                    let freq_khz = (freq_hz / 1_000).max(1);
                    let aclk_fd = self.sysctrl.r(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1) & 0xFF;
                    let aclk_khz = divide_by_fd(aclk_fd, freq_hz) / 1_000;

                    fn get_hw_time(hw: &CSR<u32>) -> u64 {
                        hw.r(utra::susres::TIME0) as u64 | ((hw.r(utra::susres::TIME1) as u64) << 32)
                    }
                    // counts_per_ms = aclk / 1000
                    // 350_000_000 / 1000 -> 350,000
                    // actual ms per count = freq / aclk, assuming freq < aclk
                    let actual_ms_per_tick = if freq_khz < 350_000 { 350_000 / aclk_khz } else { 1 };
                    // crate::println!("aclk {}kHz; ms_per_tick: {}", aclk_khz, actual_ms_per_tick);
                    let start = get_hw_time(&self.susres);
                    // round "up" by one tick to handle both the case that ms < actual_ms_per_tick
                    // and also in general this routine must guarantee at least a minimum delay but
                    // slightly longer delay is OK. This is a cheap way to not have to do precise rounding.
                    let actual_ticks = (ms as u32 / actual_ms_per_tick) + 1;
                    while get_hw_time(&self.susres) - start < actual_ticks as u64 {
                        // busy wait
                    }
                },
                false,
            )
        };
        Ok(perclk)
    }

    pub fn reboot(&mut self, soc: bool) {
        if soc {
            self.sysctrl.wo(utra::sysctrl::SFR_RCURST0, 0x0000_55aa);
        } else {
            self.sysctrl.wo(utra::sysctrl::SFR_RCURST1, 0x0000_55aa);
        }
    }

    pub fn deep_sleep(&mut self) {
        // setup wakeup mask
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x0001_003F);
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_RSTCRMASK, 0x00);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUDFTSR, 0);

        wfi_debug("entering deep sleep");
        // switch to internal osc only
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL0, 0);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL1, 1);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSET, 0x32);
        wfi_debug("internal osc");

        #[cfg(feature = "board-baosec")]
        {
            wfi_debug("disconnect dcdc2");
            self.iox.set_gpio_pin_value(self.dcdc2_io.0, self.dcdc2_io.1, bao1x_api::IoxValue::High);

            self.pmic.set_dcdc(&mut self.i2c, None, crate::axp2101::WhichDcDc::Dcdc2).unwrap();
            wfi_debug("DCDC2 off");

            self.pmic.set_dcdc(&mut self.i2c, None, crate::axp2101::WhichDcDc::Dcdc5).unwrap();
            self.pmic.set_ldo(&mut self.i2c, None, crate::axp2101::WhichLdo::Bldo2).unwrap();
            wfi_debug("camera off");

            // Turning this off probably requires disabling a bunch of interrupt handlers: it seems
            // like the system gets "activated" into some sort of .. interrupt loop? either that, or
            // there is significant leakage into an I/O from something being held up, because we see
            // *more* power rather than less when this is turned off.
            // self.pmic.set_dcdc(&mut self.i2c, None, crate::axp2101::WhichDcDc::Dcdc4).unwrap();
            // wfi_debug("DCDC4 off");
        }

        // enter PD mode - cuts VDDCORE power - requires full reset to come out of this
        // only RTC & backup regsiters are kept. ~1.2mA @ 4.2V consumption in this mode.

        // ensure use of 32k external crystal
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_CR, 7);

        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRPD, 0x0c); // immediate shutdown -- 0x0c will also turn off 2.5V domain
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRMLP0, 0x08420002); // 0.7v
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUPDAR, 0x5a);

        // deep sleep recovery by touching any button
    }

    pub fn wfi(&mut self) {
        wfi_debug("entering wfi");

        #[cfg(feature = "board-baosec")]
        {
            self.pmic.set_dcdc(&mut self.i2c, Some((0.7, true)), crate::axp2101::WhichDcDc::Dcdc2).unwrap();
            wfi_debug("DCDC2 to low voltage");

            // this might be able to save additional power - TBD
            // pmic.set_dcdc(&mut self.i2c, Some((3.0, true)), crate::axp2101::WhichDcDc::Dcdc1).unwrap();
            // wfi_debug("DCDC1 to low voltage");
        }

        // switch to internal osc only
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL0, 0);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL1, 1);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSET, 0x32);
        wfi_debug("internal osc");

        // lower core voltage to 0.7v
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420002);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
        wfi_debug("0.7v");

        // pause the ticktimer - must happen after i2c request because i2c depends on ticktimer?
        self.susres.wfo(utra::susres::CONTROL_PAUSE, 1);
        while self.susres.rf(utra::susres::STATUS_PAUSED) == 0 {}

        // power down the PLL
        self.sysctrl.wo(utra::sysctrl::SFR_IPCEN, self.sysctrl.r(utra::sysctrl::SFR_IPCEN) & !0x2);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCCR, 0x53);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        wfi_debug("PLL off");

        // ~~~~ magic code from crossbar
        // this "magic code" doesn't entirely make sense to me, and reading through the RTL
        // it probably should do approximately nothing (except for the part setting up the wakeup mask)
        // however any attempt to reform this code has lead to the system failing to come out of sleep
        // so there must be side-effects on the code that I'm not picking up reading the RTL
        // We'll just mark this as "thar be dragons" and leave the dungeon crawl to another day.
        self.sysctrl.wo(utra::sysctrl::SFR_CGULP, 0x3);
        wfi_debug("ULP");
        // ensure use of 32k extenal crystal
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_CR, 7);
        wfi_debug("CR");
        // setup wakeup mask
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x0001_003F);
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_RSTCRMASK, 0x1f);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUDFTSR, 0);
        self.ao_sysctrl
            .wo(utra::ao_sysctrl::SFR_OSCCR, self.ao_sysctrl.r(utra::ao_sysctrl::SFR_OSCCR) & !0x0001_0000);
        wfi_debug("BEF AR1");
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        wfi_debug("AFT AR1");
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSET, 0x32);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCLPEN, 0x1f);
        wfi_debug("BEF AR2");
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        wfi_debug("AFT AR2");
        // ~~~~ end magic code from crossbar

        wfi_debug("PD -> WFI");

        // this loop is useful for trying to figure out what latent interrupts were not gated off
        // diddling the top value on _i will give you a sense of how fast the interrupts are arriving and
        // when, which helps eliminate possible interrupt sources
        for _i in 0..1 {
            unsafe { core::arch::asm!("wfi", "nop", "nop", "nop", "nop") };
            // crate::println!("wakeup {}", _i);
        }
        // unsafe { core::arch::asm!("wfi", "nop", "nop", "nop", "nop") };

        wfi_debug("out of WFI");
    }

    pub fn restore_wfi(&mut self) {
        wfi_debug("enter restore_wfi");

        wfi_debug("unpause ticktimer");
        self.susres.wfo(utra::susres::CONTROL_PAUSE, 0); // this *must* be unpaused before `request_freq`

        // restore internal osc
        wfi_debug("restore osc");
        self.ao_sysctrl
            .wo(utra::ao_sysctrl::SFR_OSCCR, self.ao_sysctrl.r(utra::ao_sysctrl::SFR_OSCCR) | 0x0001_0000);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);

        // high disconnects DCDC2 from the chip - the idea is to prevent DCDC2 shutdown due to VLDO going
        // above DCDC2 value
        #[cfg(feature = "board-baosec")]
        {
            wfi_debug("disconnect dcdc2");
            self.iox.set_gpio_pin_value(self.dcdc2_io.0, self.dcdc2_io.1, bao1x_api::IoxValue::High);
        }

        // set the clock back to 350MHz CPU
        wfi_debug("request_freq");
        self.request_freq(crate::board::DEFAULT_FCLK_FREQUENCY / 1_000_000 / 2)
            .inspect_err(|e| crate::println!("req freq err {:?}", e))
            .ok();
        wfi_debug("back to 350MHz");

        #[cfg(feature = "board-baosec")]
        {
            wfi_debug("setting DCDC");

            // needed if we changed DCDC1 above
            // pmic.set_dcdc(&mut self.i2c, Some((3.3, true)), crate::axp2101::WhichDcDc::Dcdc1).unwrap();
            // wfi_debug("DCDC1 to 3.3v");

            self.pmic
                .set_dcdc(
                    &mut self.i2c,
                    Some((
                        (crate::board::DEFAULT_CPU_VOLTAGE_MV + crate::board::VDD85_SWITCH_MARGIN_MV) as f32
                            / 1000.0,
                        true,
                    )),
                    crate::axp2101::WhichDcDc::Dcdc2,
                )
                .inspect_err(|e| crate::println!("set dcdc err {:?}", e))
                .ok();

            wfi_debug("DCDC2 to normal voltage");

            // low connects DCDC2 to the chip
            self.iox.set_gpio_pin_value(self.dcdc2_io.0, self.dcdc2_io.1, bao1x_api::IoxValue::Low);
        }
    }
}

#[inline(always)]
#[cfg(feature = "std")]
fn wfi_debug(_s: &str) {
    #[cfg(feature = "debug-wfi")]
    crate::println!("{}", _s);
}
