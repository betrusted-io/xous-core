#[cfg(feature = "std")]
use bao1x_api::IoxHal;
use bitbybit::bitfield;
use utralib::*;

#[cfg(feature = "std")]
use crate::i2c::I2c;

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

    const UNIT_MHZ: u32 = 1000 * 1000;
    const PFD_F_MHZ: u32 = 16;
    const FREQ_0: u32 = 16 * UNIT_MHZ;
    const M: u32 = bao1x_api::FREQ_OSC_MHZ / PFD_F_MHZ; //  - 1;  // OSC input was 24, replace with 48

    const TBL_Q: [u16; 7] = [
        // keep later DIV even number as possible
        0x7777, // 16-32 MHz
        0x7737, // 32-64
        0x3733, // 64-128
        0x3313, // 128-256
        0x3311, // 256-512 // keep ~ 100MHz
        0x3301, // 512-1024
        0x3301, // 1024-1600
    ];
    const TBL_MUL: [u32; 7] = [
        64, // 16-32 MHz
        32, // 32-64
        16, // 64-128
        8,  // 128-256
        4,  // 256-512
        2,  // 512-1024
        2,  // 1024-1600
    ];

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
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f01_073f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x3f03_031f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x7f0f_010f); // pclk

    // calculate perclk divider. Target 100MHz.

    // perclk divider - set to divide by 16 off of an 800Mhz base. Only found on bao1x.
    // daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
    // perclk divider - set to divide by 8 off of an 800Mhz base. Only found on bao1x.
    let (min_cycle, fd, perclk) =
        if let Some((min_cycle, fd, perclk)) = bao1x_api::clk_to_per(freq_hz / 1_000_000, 100) {
            daric_cgu
                .add(utra::sysctrl::SFR_CGUFDPER.offset())
                .write_volatile((min_cycle as u32) << 16 | (fd as u32) << 8 | fd as u32);
            (min_cycle, fd, perclk * 1_000_000)
        } else if freq_hz > 400_000_000 {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x07_ff_ff);
            (7, 0xff, freq_hz / 8)
        } else {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
            (3, 0xff, freq_hz / 4)
        };

    /*
        perclk fields:  min-cycle-lp | min-cycle | fd-lp | fd
        clkper fd
            0xff :   Fperclk = Fclktop/2
            0x7f:   Fperclk = Fclktop/4
            0x3f :   Fperclk = Fclktop/8
            0x1f :   Fperclk = Fclktop/16
            0x0f :   Fperclk = Fclktop/32
            0x07 :   Fperclk = Fclktop/64
            0x03:   Fperclk = Fclktop/128
            0x01:   Fperclk = Fclktop/256

        min cycle of clktop, F means frequency
        Fperclk  Max = Fperclk/(min cycle+1)*2
    */

    // configure gates
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x00); // mbox/qfc turned off
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x02); // mdma off, sce on
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x90); // bio/udc enable
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x80); // enable mesh
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

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

    // 0: RC, 1: XTAL
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, bao1x_api::FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    if let Some(duart_ptr) = duart_base {
        let duart = duart_ptr as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        // set the ETUC now that we're on the xosc.
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(bao1x_api::FREQ_OSC_MHZ);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }

    if freq_hz <= 1_000_000 {
        cgu.wo(sysctrl::SFR_IPCOSC, freq_hz);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
    }
    // switch to OSC - now clocking off of external oscillator (glitch hazard!)
    // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    if freq_hz <= 1_000_000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        // PD PLL
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

        for _ in 0..4096 {
            bao1x_api::bollard!(4);
        }

        n_fxp24 = (((freq_hz as u64) << 24) * TBL_MUL[f16mhz_log2 as usize] as u64
            + PFD_F_MHZ as u64 * UNIT_MHZ as u64 / 2)
            / (PFD_F_MHZ as u64 * UNIT_MHZ as u64); // rounded
        let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;

        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
        cgu.wo(sysctrl::SFR_IPCPLLF, n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
        cgu.wo(sysctrl::SFR_IPCPLLQ, TBL_Q[f16mhz_log2 as usize] as u32);
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
    }
    // glitch_safety: check that we're running on the PLL
    #[cfg(not(feature = "kernel"))]
    crate::hardening::check_pll();

    crate::println!(
        "mn {:x}, q{:x}",
        (0x400400a0 as *const u32).read_volatile(),
        (0x400400a8 as *const u32).read_volatile()
    );
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

    crate::println!("Perclk solution: {:x}|{:x} -> {} MHz", min_cycle, fd, perclk / 1_000_000);
    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);

    // glitch_safety: check that we're running on the PLL
    #[cfg(not(feature = "kernel"))]
    crate::hardening::check_pll();

    perclk
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
    i2c: I2c,
    iox: IoxHal,
}

/// maximum value representable in the fd counter
#[cfg(feature = "std")]
const FD_MAX: u32 = 256;

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

        const MHZ: u32 = 1_000_000;
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

        let fclk = ClockManager::divide_by_fd(fclk_fd, pll0_freq);
        let aclk = ClockManager::divide_by_fd(aclk_fd, pll0_freq);
        let hclk = ClockManager::divide_by_fd(hclk_fd, pll0_freq);
        let iclk = ClockManager::divide_by_fd(iclk_fd, pll0_freq);
        let pclk = ClockManager::divide_by_fd(pclk_fd, pll0_freq);
        // perclk has an extra /2 applied to it
        log::info!("perclk: {:x}, pll0_freq {}", sysctrl.r(utra::sysctrl::SFR_CGUFDPER), pll0_freq);
        let perclk_fd = sysctrl.r(utra::sysctrl::SFR_CGUFDPER) & 0xFF;
        let perclk = ClockManager::divide_by_fd(perclk_fd, pll0_freq) / 2;
        Ok(Self {
            vco_freq, // 1_392_000_000,
            fclk,     // 348_000_000,
            aclk,     // 348_000_000,
            hclk,
            iclk,
            pclk,
            perclk,
            sysctrl,
            ao_sysctrl: CSR::new(ao_mem.as_ptr() as *mut u32),
            i2c: I2c::new(),
            iox: IoxHal::new(),
        })
    }

    pub fn divide_by_fd(fd: u32, in_freq_hz: u32) -> u32 {
        // shift by 1_000 to prevent overflow
        let in_freq_khz = in_freq_hz / 1_000;
        let out_freq_khz = (in_freq_khz * (fd + 1)) / FD_MAX;
        // restore to Hz
        out_freq_khz * 1_000
    }

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
                |ms: usize, _freq: u32| {
                    for i in 0..ms {
                        crate::println!("delay {}", i);
                    }
                },
                false,
            )
        };
        Ok(perclk)
    }

    pub fn wfi(&mut self) {
        crate::println!("entering wfi");

        let (port, pin) = crate::board::setup_dcdc2_pin(&self.iox);
        crate::println!("dcdc2 pin setup");
        let mut pmic = crate::axp2101::Axp2101::new(&mut self.i2c).unwrap();
        crate::println!("axp2101 handle");
        // let tt = xous_api_ticktimer::Ticktimer::new().unwrap();
        // crate::println!("tt handle");

        /*
        // high disconnects DCDC2 from the chip
        self.iox.set_gpio_pin_value(port, pin, bao1x_api::IoxValue::High);
        // we're now running on the LDO
        crate::println!("DCDC2 D/C");
        tt.sleep_ms(2).ok();

        // shut off the DCDC converter
        pmic.set_dcdc(&mut self.i2c, None, crate::axp2101::WhichDcDc::Dcdc2).unwrap();
        crate::println!("DCDC2 *off*");
        */
        /*
        crate::println!("disconnect LDOs");
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCSR, 0);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRLP, 0);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRPD, 0x0);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
        */

        // switch to internal osc only
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL0, 0);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSEL1, 1);
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSET, 0x32);
        crate::println!("internal osc");

        /*
        unsafe {
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFDAO.offset()).write_volatile(0x01010101);
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFDAORAM.offset()).write_volatile(0x01010101);
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFDPKE.offset()).write_volatile(0x01010101);
            // commit dividers
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        */

        // pmic.set_dcdc(&mut self.i2c, Some((0.70, true)), crate::axp2101::WhichDcDc::Dcdc2).unwrap();
        // crate::println!("DCDC2 to low voltage");

        // pmic.set_dcdc(&mut self.i2c, Some((2.8, true)), crate::axp2101::WhichDcDc::Dcdc1).unwrap();
        // crate::println!("DCDC1 to low voltage");

        // mbox on
        /*
        unsafe {
            self.sysctrl.base().add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xff); // mbox/qfc turned off
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        */
        // power down the PLL
        self.sysctrl.wo(utra::sysctrl::SFR_IPCEN, self.sysctrl.r(utra::sysctrl::SFR_IPCEN) & !0x2);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCCR, 0x53);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        crate::println!("PLL off");

        /*
        unsafe {
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f0f); // fclk
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f0f); // aclk
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f0f); // hclk
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f0f); // iclk
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk

            self.sysctrl.base().add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x00); // mbox/qfc turned off
            self.sysctrl.base().add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x00); // mdma off, sce on
            self.sysctrl.base().add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x00); // bio/udc enable
            self.sysctrl.base().add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x00); // enable mesh
            // commit dividers
            self.sysctrl.base().add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        */

        // lower core voltage to 0.7v
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420002);
        // self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x57);
        // crate::println!("0.7v");

        self.sysctrl.wo(utra::sysctrl::SFR_CGULP, 0x3);
        crate::println!("ULP");

        // enter PD mode - cuts VDDCORE power - requires full reset to come out of this
        // only RTC is kept (check this!). ~1.5-2mA @ 4.2V consumption in this mode.
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_CR, 7);
        crate::println!("CR");
        // setup wakeup mask
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_WKUPMASK, 0x0001_003F);
        self.ao_sysctrl.wo(utra::ao_sysctrl::CR_RSTCRMASK, 0x1f);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUDFTSR, 0);
        self.ao_sysctrl
            .wo(utra::ao_sysctrl::SFR_OSCCR, self.ao_sysctrl.r(utra::ao_sysctrl::SFR_OSCCR) & !0x0001_0000);
        crate::println!("BEF AR1");
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        crate::println!("AFT AR1");
        self.sysctrl.wo(utra::sysctrl::SFR_CGUSET, 0x32);
        self.sysctrl.wo(utra::sysctrl::SFR_IPCLPEN, 0x1f);
        crate::println!("BEF AR2");
        self.sysctrl.wo(utra::sysctrl::SFR_IPCARIPFLOW, 0x32);
        crate::println!("AFT AR2");
        // knock();
        // crate::println!("aft knock");
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUPDAR, 0x5a);
        // crate::println!("AFT PD");

        /*
        let pmuctrl = PmuControl::new_with_raw_value(0)
            .with_iout_ref_ena(true)
            .with_power_on_control(true)
            .with_pmu_2p5v_ena(true);
        // .with_pmu_0p8v_dig_ena(true);
        crate::println!("pmuctl {:x}", pmuctrl.raw_value);
        self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCSR, pmuctrl.raw_value);
        */
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRLP, 0x4c); // does nothing? we can't enter LP
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUCRPD, 0x4c); // immediate shutdown
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRMLP0, 0x08420002); // 0.7v
        // self.ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUPDAR, 0x5a);
        crate::println!("PD -> WFI");

        for _ in 0..10 {
            unsafe { core::arch::asm!("wfi", "nop", "nop", "nop", "nop") };
        }
        // unsafe { core::arch::asm!("wfi", "nop", "nop", "nop", "nop") };

        crate::println!("out of WFI");
        // tt.sleep_ms(20).ok();

        // high disconnects DCDC2 from the chip
        self.iox.set_gpio_pin_value(port, pin, bao1x_api::IoxValue::High);

        // set the clock back to 350MHz CPU
        self.request_freq(crate::board::DEFAULT_FCLK_FREQUENCY / 1_000_000 / 2)
            .inspect_err(|e| crate::println!("req freq err {:?}", e))
            .ok();
        crate::println!("back to 350MHz");

        crate::println!("setting DCDC");
        // pmic.set_dcdc(&mut self.i2c, Some((3.3, true)), crate::axp2101::WhichDcDc::Dcdc1).unwrap();
        // crate::println!("DCDC1 to 3.3v");
        pmic.set_dcdc(
            &mut self.i2c,
            Some((
                (crate::board::DEFAULT_CPU_VOLTAGE_MV + crate::board::VDD85_SWITCH_MARGIN_MV) as f32 / 1000.0,
                true,
            )),
            crate::axp2101::WhichDcDc::Dcdc2,
        )
        .inspect_err(|e| crate::println!("set dcdc err {:?}", e))
        .ok();

        crate::println!("DCDC2 to normal voltage");

        // low connects DCDC2 to the chip
        self.iox.set_gpio_pin_value(port, pin, bao1x_api::IoxValue::Low);

        /*
        // now re-enable the DCDC regulator for efficient power conversion
        // set PWM mode on DCDC2. greatly reduces noise on the regulator line
        pmic.set_pwm_mode(&mut self.i2c, crate::axp2101::WhichDcDc::Dcdc2, true).unwrap();
        // make sure the DCDC2 is set. Target 20mV above the acceptable run threshold because
        // we have to take into account the transistor loss on the
        // power switch.
        crate::println!("dcdc2 on");
        pmic.set_dcdc(
            &mut self.i2c,
            Some((
                (crate::board::DEFAULT_CPU_VOLTAGE_MV + crate::board::VDD85_SWITCH_MARGIN_MV) as f32 / 1000.0,
                true,
            )),
            crate::axp2101::WhichDcDc::Dcdc2,
        )
        .unwrap();

        crate::println!("dcdc2 connected");
        // low connects DCDC2 to the chip
        self.iox.set_gpio_pin_value(port, pin, bao1x_api::IoxValue::Low);
        */
    }
}

use core::convert::TryFrom;
pub fn knock() {
    let mut mbox = Mbox::new();

    let test_data = [0xC0DE_0000u32, 0x0000_600Du32, 0, 0, 0, 0, 0, 0];
    let mut expected_result = 0;
    for &d in test_data.iter() {
        expected_result ^= d;
    }
    let test_pkt =
        MboxToCm7Pkt { version: MBOX_PROTOCOL_REV, opcode: ToCm7Op::Knock, len: 2, data: test_data };
    // crate::println!("sending knock...\n");
    match mbox.try_send(test_pkt) {
        Ok(_) => {
            // crate::println!("Packet send Ok\n");
            let mut timeout = 0;
            while mbox.poll_not_ready() {
                timeout += 1;
                if (timeout % 1_000) == 0 {
                    crate::println!("Waiting {}...", timeout);
                }
                if timeout >= 10_000 {
                    crate::println!("Mbox timed out");
                    return;
                }
            }
            // now receive the packet
            // crate::println!("try_rx()...");
            match mbox.try_rx() {
                Ok(rx_pkt) => {
                    crate::println!("Knock result: {:x}", rx_pkt.data[0]);
                    if rx_pkt.version != MBOX_PROTOCOL_REV {
                        crate::println!("Version mismatch {} != {}", rx_pkt.version, MBOX_PROTOCOL_REV);
                    }
                    if rx_pkt.opcode != ToRvOp::RetKnock {
                        crate::println!(
                            "Opcode mismatch {} != {}",
                            rx_pkt.opcode as u16,
                            ToRvOp::RetKnock as u16
                        );
                    }
                    if rx_pkt.len != 1 {
                        crate::println!("Expected length mismatch {} != {}", rx_pkt.len, 1);
                    } else {
                        if rx_pkt.data[0] != expected_result {
                            crate::println!(
                                "Expected data mismatch {:x} != {:x}",
                                rx_pkt.data[0],
                                expected_result
                            );
                        } else {
                            crate::println!("Knock test PASS: {:x}", rx_pkt.data[0]);
                        }
                    }
                }
                Err(e) => {
                    crate::println!("Error while deserializing: {:?}\n", e);
                }
            }
        }
        Err(e) => {
            crate::println!("Packet send error: {:?}\n", e);
        }
    };
}
use utra::mailbox;

/// This constraint is limited by the size of the memory on the CM7 side
const MAX_PKT_LEN: usize = 128;
const MBOX_PROTOCOL_REV: u32 = 0;
const TX_FIFO_DEPTH: u32 = 128;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum MboxError {
    None,
    NotReady,
    TxOverflow,
    TxUnderflow,
    RxOverflow,
    RxUnderflow,
    InvalidOpcode,
    AbortFailed,
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToRvOp {
    Invalid = 0,

    RetKnock = 128,
    RetDct8x8 = 129,
    RetClifford = 130,
}
impl TryFrom<u16> for ToRvOp {
    type Error = MboxError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ToRvOp::Invalid),
            128 => Ok(ToRvOp::RetKnock),
            129 => Ok(ToRvOp::RetDct8x8),
            130 => Ok(ToRvOp::RetClifford),
            _ => Err(MboxError::InvalidOpcode),
        }
    }
}

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ToCm7Op {
    Invalid = 0,

    Knock = 1,
    Dct8x8 = 2,
    Clifford = 3,
}

const STATIC_DATA_LEN: usize = 8;
pub struct MboxToCm7Pkt {
    version: u32,
    opcode: ToCm7Op,
    len: usize,
    data: [u32; STATIC_DATA_LEN],
}

pub struct MboxToRvPkt {
    version: u32,
    opcode: ToRvOp,
    len: usize,
    data: [u32; STATIC_DATA_LEN],
}

pub struct Mbox {
    csr: CSR<u32>,
}
impl Mbox {
    pub fn new() -> Mbox {
        let mem = xous::map_memory(
            xous::MemoryAddress::new(mailbox::HW_MAILBOX_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .unwrap();

        Self { csr: CSR::new(mem.as_ptr() as *mut u32) }
    }

    fn expect_tx(&mut self, val: u32) -> Result<(), MboxError> {
        if (TX_FIFO_DEPTH - self.csr.rf(mailbox::STATUS_TX_WORDS)) == 0 {
            return Err(MboxError::TxOverflow);
        } else {
            self.csr.wo(mailbox::WDATA, val);
            Ok(())
        }
    }

    pub fn try_send(&mut self, to_cm7: MboxToCm7Pkt) -> Result<(), MboxError> {
        // clear any pending bits from previous transactions
        self.csr.wo(mailbox::EV_PENDING, self.csr.r(mailbox::EV_PENDING));

        if to_cm7.len > MAX_PKT_LEN {
            Err(MboxError::TxOverflow)
        } else {
            self.expect_tx(to_cm7.version)?;
            self.expect_tx(to_cm7.opcode as u32 | (to_cm7.len as u32) << 16)?;
            for &d in to_cm7.data[..to_cm7.len].iter() {
                self.expect_tx(d)?;
            }
            // trigger the send
            self.csr.wfo(mailbox::DONE_DONE, 1);
            Ok(())
        }
    }

    fn expect_rx(&mut self) -> Result<u32, MboxError> {
        if self.csr.rf(mailbox::STATUS_RX_WORDS) == 0 {
            Err(MboxError::RxUnderflow)
        } else {
            Ok(self.csr.r(mailbox::RDATA))
        }
    }

    pub fn try_rx(&mut self) -> Result<MboxToRvPkt, MboxError> {
        let version = self.expect_rx()?;
        let op_and_len = self.expect_rx()?;
        let opcode = ToRvOp::try_from((op_and_len & 0xFFFF) as u16)?;
        let len = (op_and_len >> 16) as usize;
        let mut data = [0u32; STATIC_DATA_LEN];
        for d in data[..len.min(STATIC_DATA_LEN)].iter_mut() {
            *d = self.expect_rx()?;
        }
        Ok(MboxToRvPkt { version, opcode, len, data })
    }

    pub fn poll_not_ready(&self) -> bool { self.csr.rf(mailbox::EV_PENDING_AVAILABLE) == 0 }

    pub fn abort(&mut self) -> Result<(), MboxError> {
        crate::println!("Initiating abort");
        self.csr.wfo(utra::mailbox::CONTROL_ABORT, 1);
        const TIMEOUT: usize = 1000;
        for _ in 0..TIMEOUT {
            if self.csr.rf(utra::mailbox::STATUS_ABORT_IN_PROGRESS) == 0 {
                return Ok(());
            }
        }
        return Err(MboxError::AbortFailed);
    }
}
