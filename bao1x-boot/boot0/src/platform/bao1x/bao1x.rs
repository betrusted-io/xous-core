use bao1x_api::bollard;
use bao1x_api::signatures::SIGBLOCK_LEN;
use bao1x_hal::acram::OneWayCounter;
use bao1x_hal::hardening::{Csprng, die, paranoid_mode};
use digest::Digest;
use sha2_bao1x::Sha512;
use utralib::CSR;
use utralib::utra;
use utralib::utra::sysctrl;

#[cfg(feature = "unsafe-dev")]
use crate::platform::debug::setup_rx;
use crate::platform::irq::{enable_irq, irq_setup};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;

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

/// Run at 350MHz to ensure we can boot even without an external VDD85 regulator!
/// Also relying on the IFR region setting the SRAM trimming to work at this safe default
/// so we don't have to initialize it in boot0.
pub const DEFAULT_FCLK_FREQUENCY: u32 = 350_000_000;

pub fn early_init() -> Csprng {
    // glitch_safety: set up an initial random delay so that the later routines aren't executing
    // so predictably. This uses a raw, untested TRNG; later on, a safer version of this is created
    // but at this point we're running at only 32 MHz or so, so we don't have a lot of gas to do
    // fancy things.
    let mut ro_trng = bao1x_hal::sce::trng::Trng::new(utra::trng::HW_TRNG_BASE);
    ro_trng.setup_raw_generation(32);
    let delay = ro_trng.get_raw() & 0xFFF;
    // this should insert a delay of around 0-12 microseconds
    for _ in 0..delay {
        unsafe { core::arch::asm!("nop") };
    }

    // This checks if paranoid mode should be entered. If we should enter it, the system will
    // automatically reset on any glitch detection attempt.

    // === FIRST read of paranoid mode ===
    bollard!(die, 4);
    let owc = OneWayCounter::new();
    let (paranoid1, paranoid2) =
        owc.hardened_get2(bao1x_api::PARANOID_MODE, bao1x_api::PARANOID_MODE_DUPE).unwrap();
    bollard!(die, 4);
    if paranoid1 != paranoid2 {
        die();
    }
    bollard!(die, 4);
    if paranoid1 != 0 {
        bollard!(die, 4);
        paranoid_mode();
    }
    // this should insert a delay of around 0-12 microseconds
    let delay = ro_trng.get_raw() & 0xFFF;
    for _ in 0..delay {
        bollard!(die, 4);
    }
    bollard!(die, 4);
    if paranoid2 != 0 {
        bollard!(die, 4);
        paranoid_mode();
    }

    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    // glitch_safety: the next set of code, if not run correctly, will lead to non-functioning hardware
    // the behavior might be exploitable but probably more likely to be undefined/unpredictable
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
    // glitch_safety: if the statics aren't set up correctly, you generally end up with code
    // hangs because most of the statics set up locks to unlocked states, and missing this will
    // cause them to initialize in a lock state.
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
            data_ptr
                .add(u16::from_le_bytes(offset) as usize / size_of::<u32>())
                .write_volatile(u32::from_le_bytes(data));
        }
    }

    // set the clock.
    // glitch_safety: this code must be hardened, as disabling the PLL and running directly off
    // external clock could make the chip very exploitable. See inside the function for details.
    let perclk = unsafe { init_clock_asic_350mhz() };

    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);
    // the CSPRNG is built *after* we go to high speed mode. Note the discipline that after every
    // print, we insert a random_delay - so that you can't use the print as a reliable trigger.
    let mut csprng = Csprng::new();
    csprng.random_delay();

    irq_setup();
    // set up the reactive sensors
    bao1x_api::bollard!(4);
    bao1x_hal::hardening::reset_sensors();
    bao1x_api::bollard!(4);
    bollard!(die, 4);
    csprng.random_delay();
    // ensure paranoid mode is respected
    if paranoid2 != 0 {
        bollard!(die, 4);
        paranoid_mode();
    }
    // set up IRQ to respond to all possible sensor errors
    let mut irq13 = CSR::new(utra::irqarray13::HW_IRQARRAY13_BASE as *mut u32);
    irq13.wo(utra::irqarray13::EV_EDGE_TRIGGERED, 0xFFFF_FFFF);
    irq13.wo(utra::irqarray13::EV_POLARITY, 0xFFFF_FFFF);
    irq13.wo(utra::irqarray13::EV_PENDING, 0xFFFF_FFFF);
    irq13.wo(utra::irqarray13::EV_ENABLE, 0xFFFF_FFFF);
    enable_irq(utra::irqarray13::IRQARRAY13_IRQ);
    csprng.random_delay();
    if paranoid1 != 0 {
        bollard!(die, 4);
        paranoid_mode();
    }

    // setup heap alloc
    // glitch_safety: failing to initialize this will generally cause heap allocs to hang
    setup_alloc();

    // glitch_safety: failing to initialize this will mostly lead to unpredictable delays in
    // non-security critical code
    setup_timer();

    // glitch_safety: redundant check to make sure we're still using the PLL, and that a glitch
    // in `init_clock_asic_350mhz` wasn't able to skip over the switch to PLL mode. This check
    // in combination with the previous check inside init_clock_asic_350mhz means you need
    // three successful glitches to bypass these checks: one to skip PLL setting, and one each
    // for the checks. There is also a random_delay() inserted between the previous check and now.
    bao1x_api::bollard!(4);
    bao1x_hal::hardening::check_pll();
    bao1x_api::bollard!(4);

    // this is a security-critical initialization. Failure to do this correctly breaks
    // all the hardware hashes. It's done once, in boot0. Note that the constants *are*
    // malleable (no hardware lock to prevent update), which is a potential vulnerability.
    //
    // glitch_safety: in terms of glitching, having the hash round constants set to random
    // values breaks the signature check in unpredictable ways. It's not clear that writing
    // this twice improves glitch safety, because you can always just glitch the second
    // write (it's a big operation, 10k of data written). So, we just do it once, without
    // any particular hardening.
    init_hash();

    // SHA-512 Known Answer Test (KAT)
    const TEST_SHA512_DATA: &[u8] = b"0NlY  TH3   50Urce   c@n 5et You fr3e";
    const EXPECTED_HASH: [u8; 64] = hex_literal::hex!(
        "00000007a8d4c9f11f6ad8a8d71aa73a53c7ac392098f7a731b159d50586d7d08e5174218dbb2eaa7c9599165e6746e199410b9a86e74840052afa23e4976189"
    );
    let mut hasher = Sha512::new();
    hasher.update(TEST_SHA512_DATA);
    let digest: [u8; 64] = hasher.finalize().try_into().unwrap();
    if digest != EXPECTED_HASH {
        die();
    }

    // TxRx setup
    #[cfg(feature = "unsafe-dev")]
    let mut udma_uart = {
        let mut udma_uart = setup_rx(perclk);
        enable_irq(utra::irqarray5::IRQARRAY5_IRQ);
        udma_uart
    };
    // Tx-only setup
    #[cfg(not(feature = "unsafe-dev"))]
    let _udma_uart = crate::debug::setup_tx(perclk);

    // glitch_safety: this contant isn't security-critical
    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);
    crate::println!("boot0 console up");
    csprng.random_delay();

    csprng
}

pub fn setup_timer() {
    // Initialize the timer, which is needed by the delay() function.
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    // not using interrupts, this will be polled by delay()
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);

    let ms = 1;
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
    bao1x_api::bollard!(4);
    for (dst, &src) in sce_mem.iter_mut().zip(constants) {
        *dst = src;
    }
    bao1x_api::bollard!(4);
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

/// The frequency parameter is hard-coded for this implementation because running at a higher
/// speed makes glitches more difficult to time. We also don't want any risk of a glitch bypassing
/// the PLL.
pub unsafe fn init_clock_asic_350mhz() -> u32 {
    let freq_hz = 350_000_000;
    use utra::sysctrl;
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    let mut cgu = CSR::new(daric_cgu);

    // Safest divider settings, assuming no overclocking.
    // If overclocking, need to lower hclk:iclk:pclk even futher; the CPU speed can outperform the bus fabric.
    // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
    // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f7f); // fclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
    daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk

    // hard coded
    daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x009191);
    let perclk = 100_000_000;

    // turn off gates
    daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x00); // mbox/qfc turned off
    daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0x02); // mdma off, sce on
    daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x90); // bio/udc enable
    daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0x80); // enable mesh
    // commit dividers
    daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);

    let mut ao_sysctrl = CSR::new(utralib::HW_AO_SYSCTRL_BASE as *mut u32);
    ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM0CSR, 0x08420420);
    ao_sysctrl.wo(utra::ao_sysctrl::SFR_PMUTRM1CSR, 0x2);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x57);
    crate::platform::delay_at_sysfreq(20, 48_000_000);

    // glitch_safety: this switches us to the external crystal for a period of time to configure
    // the PLL. I don't think there is an option around this - it does mean we have a risk of a glitch
    // preventing a switch back, so the switch-back has to be hardened.
    cgu.wo(sysctrl::SFR_CGUSEL1, 1);
    cgu.wo(sysctrl::SFR_CGUFSCR, bao1x_api::FREQ_OSC_MHZ);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    let duart = utra::duart::HW_DUART_BASE as *mut u32;
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(0);
    // set the ETUC now that we're on the xosc.
    duart.add(utra::duart::SFR_ETUC.offset()).write_volatile(bao1x_api::FREQ_OSC_MHZ);
    duart.add(utra::duart::SFR_CR.offset()).write_volatile(1);

    // switch to OSC
    cgu.wo(sysctrl::SFR_CGUSEL0, 0);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);

    // PD PLL
    cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) | 0x2);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    for _ in 0..4096 {
        bao1x_api::bollard!(4);
    }

    // hard-coded so that there's less chances for glitches to muck with PLL parameters
    cgu.wo(sysctrl::SFR_IPCPLLMN, 0x3057);
    cgu.wo(sysctrl::SFR_IPCPLLF, 0x1800000);
    cgu.wo(sysctrl::SFR_IPCPLLQ, 0x3311);
    cgu.wo(sysctrl::SFR_IPCCR, 0x53);

    bao1x_api::bollard!(6);
    // written twice - this is safe to do, because values don't take hold until ARIPFLOW is triggered
    cgu.wo(sysctrl::SFR_IPCPLLMN, 0x3057);
    cgu.wo(sysctrl::SFR_IPCPLLF, 0x1800000);
    cgu.wo(sysctrl::SFR_IPCPLLQ, 0x3311);
    cgu.wo(sysctrl::SFR_IPCCR, 0x53);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    cgu.wo(sysctrl::SFR_IPCLPEN, cgu.r(sysctrl::SFR_IPCLPEN) & !0x2);
    cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);

    for _ in 0..4096 {
        bao1x_api::bollard!(4);
    }

    // place bollards around the PLL re-enable code.
    bao1x_api::bollard!(6);
    cgu.wo(sysctrl::SFR_CGUSEL0, 1);
    cgu.wo(sysctrl::SFR_CGUSET, 0x32);
    bao1x_api::bollard!(6);

    // UDMA subsystem controls its own gates
    let mut udmacore = CSR::new(utra::udma_ctrl::HW_UDMA_CTRL_BASE as *mut u32);
    udmacore.wo(utra::udma_ctrl::REG_CG, 0x0);

    // glitch_safety: check that we're running on the PLL
    bao1x_hal::hardening::check_pll();

    // pll print is put after the routine, so it can't be used as a glitch trigger
    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);

    /*
    // these are the prints used to instrument the normal clock setting flow to derive the constants
    // used in this section.

    crate::println!(
        "min_cycle {}, fd {}, perclk {}, SFR_CGUFDPER: {:x}",
        min_cycle,
        fd,
        perclk,
        (min_cycle as u32) << 16 | (fd as u32) << 8 | fd as u32
    );
    crate::println!("freq_hz {} log2 {}", freq_hz, f16mhz_log2);
    crate::println!("SFR_IPCPLLMN: {:x}", ((M << 12) & 0x0001F000) | (((n_fxp24 >> 24) as u32) & 0x00000fff));
    crate::println!("SFR_IPCPLLF: {:x}", n_frac | if 0 == n_frac { 0 } else { 1u32 << 24 });
    crate::println!("SFR_IPCPLLQ: {:x}", TBL_Q[f16mhz_log2 as usize] as u32);
    crate::println!("SFR_IPCCR: {:x}", (1 << 6) | (2 << 3) | (3));

    Results @ 350MHz:
        min_cycle 1, fd 145, perclk 175000000, SFR_CGUFDPER: 19191
        freq_hz 350000000 log2 4
        SFR_IPCPLLMN: 3057
        SFR_IPCPLLF: 1800000
        SFR_IPCPLLQ: 3311
        SFR_IPCCR: 53
    */

    perclk
}
