use utralib::*;

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
pub unsafe fn init_clock_asic(
    freq_hz: u32,
    cgu_base: usize,
    ao_sysctl_base: usize,
    duart_base: Option<usize>,
    delay_at_sysfreq: fn(usize, u32),
    fast_bio: bool,
) -> u32 {
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
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3fff); // fclk
    } else {
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f7f); // fclk
    }
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk

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
    // Comment this out - I think we manage the UDMA clocks explicitly per-driver
    // let mut udmacore = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    // udmacore.wo(utra::udma_ctrl::REG_CG, 0xFFFF_FFFF);

    crate::println!("Perclk solution: {:x}|{:x} -> {} MHz", min_cycle, fd, perclk / 1_000_000);
    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);

    // glitch_safety: check that we're running on the PLL
    #[cfg(not(feature = "kernel"))]
    crate::hardening::check_pll();

    perclk
}
