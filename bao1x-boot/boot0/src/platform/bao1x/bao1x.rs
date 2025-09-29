use bao1x_api::signatures::SIGBLOCK_LEN;
use utralib::CSR;
use utralib::utra;
use utralib::utra::sysctrl;

#[cfg(feature = "unsafe-dev")]
use crate::platform::{
    debug::setup_rx,
    irq::{enable_irq, irq_setup},
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;

// This may not be a great assumption. TODO: fix this by deriving from the static boot constants.
// also fix this in the baremetal/loader configs.
const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

// scratch page for exceptions
//   - scratch data is stored in positive offsets from here
//   - exception stack is stored in negative offsets from here, hence the +4096
// total occupied area is [HEAP_START + HEAP_LEN..HEAP_START + HEAP_LEN + 8192]
pub const SCRATCH_PAGE: usize = HEAP_START + HEAP_LEN + 4096;

pub const UART_IFRAM_ADDR: usize = bao1x_hal::board::UART_DMA_TX_BUF_PHYS;

/// Run at 400MHz to ensure we can boot even without an external VDD85 regulator!
/// Also relying on the IFR region setting the SRAM trimming to work at this safe default
/// so we don't have to initialize it in boot0.
pub const DEFAULT_FCLK_FREQUENCY: u32 = 400_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

pub fn early_init() {
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;

    unsafe {
        // this block is mandatory in all cases to get clocks set into some consistent, expected mode
        {
            // conservative dividers
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x7f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x3f7f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x1f3f);
            daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x0f1f);
            // ungate all clocks
            daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xFF);
            daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xFF);
            // commit clocks
            daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        }
        // enable DUART
        let duart = utra::duart::HW_DUART_BASE as *mut u32;
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
        // based on ringosc trimming as measured by oscope. this will get precise after we set the PLL.
        duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(34);
        duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);
    }
    // sram0 requires 1 wait state for writes
    let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM0, 0x8);

    // Now that SRAM trims are setup, initialize all the statics by writing to memory.
    // For baremetal, the statics structure is just at the flash base.
    const STATICS_LOC: usize = bao1x_api::BOOT0_START + SIGBLOCK_LEN;

    // safety: this data structure is pre-loaded by the image loader and is guaranteed to
    // only have representable, valid values that are aligned according to the repr(C) spec
    let statics_in_rom: &bao1x_api::StaticsInRom =
        unsafe { (STATICS_LOC as *const bao1x_api::StaticsInRom).as_ref().unwrap() };
    assert!(statics_in_rom.version == bao1x_api::STATICS_IN_ROM_VERSION, "Can't find valid statics table");

    // Clear .data, .bss, .stack, .heap regions & setup .data values
    // Safety: only safe if the values computed by the loader are correct.
    unsafe {
        let data_ptr = statics_in_rom.data_origin as *mut u32;
        for i in 0..statics_in_rom.data_size_bytes as usize / size_of::<u32>() {
            data_ptr.add(i).write_volatile(0);
        }
        for &(offset, data) in &statics_in_rom.poke_table[..statics_in_rom.valid_pokes as usize] {
            data_ptr.add(offset as usize).write_volatile(data);
        }
    }

    // set the clock.
    let perclk = unsafe { init_clock_asic(DEFAULT_FCLK_FREQUENCY) };

    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);

    // setup heap alloc
    setup_alloc();

    setup_timer();

    // this is a security-critical initialization. Failure to do this correctly breaks
    // all the hardware hashes. It's done once, in boot0. Note that the constants *are*
    // malleable (no hardware lock to prevent update), which is a potential vulnerability.
    init_hash();

    // TxRx setup
    #[cfg(feature = "unsafe-dev")]
    let mut udma_uart = {
        let mut udma_uart = setup_rx(perclk);
        irq_setup();
        enable_irq(utra::irqarray5::IRQARRAY5_IRQ);
        udma_uart
    };
    // Tx-only setup
    #[cfg(not(feature = "unsafe-dev"))]
    let _udma_uart = crate::debug::setup_tx(perclk);

    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);
    crate::println!("boot0 console up");
}

pub fn setup_timer() {
    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    // not using interrupts, this will be polled by delay()
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);

    let ms = SYSTEM_TICK_INTERVAL_MS;
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    // load its values
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, (DEFAULT_FCLK_FREQUENCY / 1_000) * ms);
    // enable the timer
    timer.wfo(utra::timer0::EN_EN, 0b1);
}

pub fn setup_alloc() {
    // Initialize the allocator with heap memory range
    crate::println!("Setting up heap @ {:x}-{:x}", HEAP_START, HEAP_START + HEAP_LEN);
    crate::println!("Stack @ {:x}-{:x}", HEAP_START + HEAP_LEN, RAM_BASE + RAM_SIZE);
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}

/// This function loads all the round constants into the combohasher's local memory.
pub fn init_hash() {
    use bao1x_api::sce::combohash::*;
    // safety: this one is a little less clear from the register set extraction. But in the case of
    // system initialization, the SCE uses its entire RAM range (10kiB worth) as a single buffer, so
    // none of the buffer boundaries are respected. Thus we set the length of the segment to 10kiB
    // solely because as hardware designers, we know this is what's there. You can see the size of
    // the SCERAM's block definition via its RBIST wrapper here:
    // https://github.com/baochip/baochip-1x/blob/96ba390759ba361e50e57bd21f02c806ddafc4ff/rtl/modules/soc_coresub/rtl/soc_coresub.sv#L1018
    let sce_mem = unsafe {
        core::slice::from_raw_parts_mut(utralib::HW_SEG_LKEY_MEM as *mut u32, 10 * 1024 / size_of::<u32>())
    };
    #[rustfmt::skip]
    let constants =
        SHA256_H.iter().chain(
        SHA256_K.iter().chain(
        SHA512_H.iter().chain(
        SHA512_K.iter().chain(
        BLK2S_H.iter().chain(
        BLK2B_H.iter().chain(
        BLK2_X.iter().chain(
        BLK3_H.iter().chain(
        BLK3_X.iter().chain(
        RIPMD_H.iter().chain(
        RIPMD_K.iter().chain(
        RIPMD_X.iter().chain(
        RAMSEG_SHA3.iter()
    ))))))))))));
    for (dst, &src) in sce_mem.iter_mut().zip(constants) {
        *dst = src;
    }
    let mut combo_hash = CSR::new(utra::combohash::HW_COMBOHASH_BASE as *mut u32);
    combo_hash.wo(utra::combohash::SFR_OPT3, 0); // u32 big-endian constant load
    combo_hash.wfo(utra::combohash::SFR_CRFUNC_CR_FUNC, HashFunction::Init as u32);

    combo_hash.wo(utra::combohash::SFR_FR, 0xf); // clear completion flag
    combo_hash.wo(utra::combohash::SFR_AR, 0x5a); // start
    while combo_hash.rf(utra::combohash::SFR_FR_MFSM_DONE) == 0 {
        // wait for mem to copy
    }
    // clear the flag on exit
    combo_hash.rmwf(utra::combohash::SFR_FR_MFSM_DONE, 1);
}

/// Delay with a given system clock frequency. Useful during power mode switching.
pub fn delay_at_sysfreq(ms: usize, sysclk_freq: u32) {
    let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EN_EN, 0b0); // disable the timer
    timer.wfo(utra::timer0::LOAD_LOAD, 0);
    timer.wfo(utra::timer0::RELOAD_RELOAD, sysclk_freq / 1000);
    timer.wfo(utra::timer0::EN_EN, 1);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        // comment this out for testing on MPW
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}

/// Delay function that delays a given number of milliseconds.
#[allow(dead_code)]
pub fn delay(ms: usize) {
    let mut timer = utralib::CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        // comment this out for testing on MPW
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}

mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

/// Takes in the top clock in MHz, desired perclk in MHz, and returns a tuple of
/// (min cycle, fd, actual freq)
/// *tested*
pub fn clk_to_per(top_in_mhz: u32, perclk_in_mhz: u32) -> Option<(u8, u8, u32)> {
    let fd_platonic = ((256 * perclk_in_mhz) / (top_in_mhz / 2)).min(256);
    if fd_platonic > 0 {
        let fd = fd_platonic - 1;
        let min_cycle = (2 * (256 / (fd + 1))).max(1);
        let min_freq = top_in_mhz / min_cycle;
        let target_freq = top_in_mhz * (fd + 1) / 512;
        let actual_freq = target_freq.max(min_freq);
        if fd < 256 && min_cycle < 256 && min_cycle > 0 {
            Some(((min_cycle - 1) as u8, fd as u8, actual_freq))
        } else {
            None
        }
    } else {
        None
    }
}

/// TODO: pare this down for the boot0 function
pub unsafe fn init_clock_asic(freq_hz: u32) -> u32 {
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    const UNIT_MHZ: u32 = 1000 * 1000;
    const PFD_F_MHZ: u32 = 16;
    const FREQ_0: u32 = 16 * UNIT_MHZ;
    const FREQ_OSC_MHZ: u32 = 48; // Actually 48MHz
    const M: u32 = FREQ_OSC_MHZ / PFD_F_MHZ; //  - 1;  // OSC input was 24, replace with 48

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
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f7f); // fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk

    // calculate perclk divider. Target 100MHz.

    // perclk divider - set to divide by 16 off of an 800Mhz base. Only found on bao1x.
    // daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
    // perclk divider - set to divide by 8 off of an 800Mhz base. Only found on bao1x.
    // TODO: this only works for two clock settings. Broken @ 600. Need to fix this to instead derive
    // what pclk is instead of always reporting 100mhz
    let (_min_cycle, _fd, perclk) =
        if let Some((min_cycle, fd, perclk)) = clk_to_per(freq_hz / 1_000_000, 100) {
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

    // turn off gates
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0xff);
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xff);
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

    if freq_hz >= 600_000_000 {
        crate::println!("setting vdd85 to 0.893v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421FF1);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    } else {
        crate::println!("setting vdd85 to 0.80v");
        let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08421080);
        ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
        crate::platform::delay_at_sysfreq(20, 48_000_000);
    }
    crate::println!("...done");

    // DARIC_CGU->cgusel1 = 1; // 0: RC, 1: XTAL
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    // DARIC_CGU->cgufscr = FREQ_OSC_MHZ; // external crystal is 48MHz
    cgu.wo(sysctrl::SFR_CGUFSCR, FREQ_OSC_MHZ);
    // __DSB();
    // DARIC_CGU->cguset = 0x32UL;
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    // __DSB();

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    if freq_hz <= 1_000_000 {
        // DARIC_IPC->osc = freqHz;
        cgu.wo(sysctrl::SFR_IPCOSC, freq_hz);
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();
    }
    // switch to OSC
    //DARIC_CGU->cgusel0 = 0; // clktop sel, 0:clksys, 1:clkpll0
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    // __DSB();
    // DARIC_CGU->cguset = 0x32; // commit
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    //__DSB();

    if freq_hz <= 1_000_000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        // PD PLL
        // DARIC_IPC->lpen |= 0x02 ;
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit, must write 32
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        // for (uint32_t i = 0; i < 1024; i++){
        //    __NOP();
        //}
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 1");
        // why is this print needed for the code not to crash?
        crate::println!("freq_hz {} log2 {}", freq_hz, f16mhz_log2);
        n_fxp24 = (((freq_hz as u64) << 24) * TBL_MUL[f16mhz_log2 as usize] as u64
            + PFD_F_MHZ as u64 * UNIT_MHZ as u64 / 2)
            / (PFD_F_MHZ as u64 * UNIT_MHZ as u64); // rounded
        let n_frac: u32 = (n_fxp24 & 0x00ffffff) as u32;

        // TODO very verbose
        //printf ("%s(%4" PRIu32 "MHz) M = %4" PRIu32 ", N = %4" PRIu32 ".%08" PRIu32 ", Q = %2lu, QDiv =
        // 0x%04" PRIx16 "\n",     __FUNCTION__, freqHz / 1000000, M, (uint32_t)(n_fxp24 >> 24),
        // (uint32_t)((uint64_t)(n_fxp24 & 0x00ffffff) * 100000000/(1UL <<24)), TBL_MUL[f16MHzLog2],
        // TBL_Q[f16MHzLog2]); DARIC_IPC->pll_mn = ((M << 12) & 0x0001F000) | ((n_fxp24 >> 24) &
        // 0x00000fff); // 0x1F598; // ??
        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
        // DARIC_IPC->pll_f = n_frac | ((0 == n_frac) ? 0 : (1UL << 24)); // ??
        cgu.wo(sysctrl::SFR_IPCPLLF, n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
        // DARIC_IPC->pll_q = TBL_Q[f16MHzLog2]; // ?? TODO select DIV for VCO freq
        cgu.wo(sysctrl::SFR_IPCPLLQ, TBL_Q[f16mhz_log2 as usize] as u32);
        //               VCO bias   CPP bias   CPI bias
        //                1          2          3
        //DARIC_IPC->ipc = (3 << 6) | (5 << 3) | (5);
        // DARIC_IPC->ipc = (1 << 6) | (2 << 3) | (3);
        cgu.wo(sysctrl::SFR_IPCCR, (1 << 6) | (2 << 3) | (3));
        // __DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // DARIC_IPC->lpen &= ~0x02;
        cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);

        //__DSB();
        // DARIC_IPC->ar     = 0x0032;  // commit
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
        // __DSB();

        // delay
        // for (uint32_t i = 0; i < 1024; i++){
        //    __NOP();
        // }
        for _ in 0..1024 {
            unsafe { core::arch::asm!("nop") };
        }
        crate::println!("PLL delay 2");

        // DARIC_CGU->cgusel0 = 1; // clktop sel, 0:clksys, 1:clkpll0
        cgu.wo(sysctrl::SFR_CGUSEL0, 1);
        // __DSB();
        // DARIC_CGU->cguset = 0x32; // commit
        cgu.wo(sysctrl::SFR_CGUSET, 0x32);
        crate::println!("clocks set");
    }

    // Taken in from latest daric_util.c
    let mut udmacore = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udmacore.wo(utra::udma_ctrl::REG_CG, 0xFFFF_FFFF);

    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);
    perclk
}
