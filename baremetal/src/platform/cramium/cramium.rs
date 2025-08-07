use utralib::CSR;
use utralib::utra;
use utralib::utra::sysctrl;

use crate::platform::{
    debug::setup_rx,
    irq::{enable_irq, irq_setup},
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN - 0x8_0000;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM + 0x8_0000;
#[allow(dead_code)]
pub const FLASH_BASE: usize = utralib::generated::HW_RERAM_MEM;

pub const HEAP_START: usize = RAM_BASE + 0x5000;
pub const HEAP_LEN: usize = 1024 * 256;

// scratch page for exceptions located at top of RAM
// NOTE: there is an additional page above this for exception stack
pub const SCRATCH_PAGE: usize = HEAP_START - 8192;

pub const UART_IFRAM_ADDR: usize = cramium_hal::board::UART_DMA_TX_BUF_PHYS;

// the 800_000_000 setting is tested to work at least on one sample
pub const SYSTEM_CLOCK_FREQUENCY: u32 = 400_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

pub fn early_init() {
    let daric_cgu = sysctrl::HW_SYSCTRL_BASE as *mut u32;
    /*
    let ao_sysctrl = utra::ao_sysctrl::HW_AO_SYSCTRL_BASE as *mut u32;
    unsafe {
        // this turns off the VDD85D (doesn't work)
        // ao_sysctrl.add(utra::ao_sysctrl::SFR_PMUCSR.offset()).write_volatile(0x6c);

        // this sets VDD85D to 0.90V
        ao_sysctrl.add(utra::ao_sysctrl::SFR_PMUTRM0CSR.offset()).write_volatile(0x0842_10E0); // 0x0842_1080 default
        daric_cgu.add(utra::sysctrl::SFR_IPCARIPFLOW.offset()).write_volatile(0x57);
    }
    */
    let uart = crate::debug::Uart {};

    for _ in 0..100 {
        uart.putc('a' as u32 as u8);
    }
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
    sramtrm.wo(utra::coresub_sramtrm::SFR_SRAM1, 0x8);

    #[cfg(feature = "v0p9")]
    {
        /*
        logic [15:0] trm_ram32kx72      ; assign trm_ram32kx72      = trmdat[0 ]; localparam t_trm IV_trm_ram32kx72      = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_ram8kx72       ; assign trm_ram8kx72       = trmdat[1 ]; localparam t_trm IV_trm_ram8kx72       = IV_sram_sp_hde_inst_sram1;
        logic [15:0] trm_rf1kx72        ; assign trm_rf1kx72        = trmdat[2 ]; localparam t_trm IV_trm_rf1kx72        = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf256x27       ; assign trm_rf256x27       = trmdat[3 ]; localparam t_trm IV_trm_rf256x27       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf512x39       ; assign trm_rf512x39       = trmdat[4 ]; localparam t_trm IV_trm_rf512x39       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_rf128x31       ; assign trm_rf128x31       = trmdat[5 ]; localparam t_trm IV_trm_rf128x31       = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_dtcm8kx36      ; assign trm_dtcm8kx36      = trmdat[6 ]; localparam t_trm IV_trm_dtcm8kx36      = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_itcm32kx18     ; assign trm_itcm32kx18     = trmdat[7 ]; localparam t_trm IV_trm_itcm32kx18     = IV_sram_sp_hde_inst_tcm;
        logic [15:0] trm_ifram32kx36    ; assign trm_ifram32kx36    = trmdat[8 ]; localparam t_trm IV_trm_ifram32kx36    = IV_sram_sp_uhde_inst;
        logic [15:0] trm_sce_sceram_10k ; assign trm_sce_sceram_10k = trmdat[9 ]; localparam t_trm IV_trm_sce_sceram_10k = IV_sram_sp_hde_inst;
        logic [15:0] trm_sce_hashram_3k ; assign trm_sce_hashram_3k = trmdat[10]; localparam t_trm IV_trm_sce_hashram_3k = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aesram_1k  ; assign trm_sce_aesram_1k  = trmdat[11]; localparam t_trm IV_trm_sce_aesram_1k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_pkeram_4k  ; assign trm_sce_pkeram_4k  = trmdat[12]; localparam t_trm IV_trm_sce_pkeram_4k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_aluram_3k  ; assign trm_sce_aluram_3k  = trmdat[13]; localparam t_trm IV_trm_sce_aluram_3k  = IV_rf_sp_hde_inst;
        logic [15:0] trm_sce_mimmdpram  ; assign trm_sce_mimmdpram  = trmdat[14]; localparam t_trm IV_trm_sce_mimmdpram  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rdram1kx32     ; assign trm_rdram1kx32     = trmdat[15]; localparam t_trm IV_trm_rdram1kx32     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram512x64    ; assign trm_rdram512x64    = trmdat[16]; localparam t_trm IV_trm_rdram512x64    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram128x22    ; assign trm_rdram128x22    = trmdat[17]; localparam t_trm IV_trm_rdram128x22    = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_rdram32x16     ; assign trm_rdram32x16     = trmdat[18]; localparam t_trm IV_trm_rdram32x16     = IV_rf_2p_hdc_inst_vex;
        logic [15:0] trm_bioram1kx32    ; assign trm_bioram1kx32    = trmdat[19]; localparam t_trm IV_trm_bioram1kx32    = IV_rf_sp_hde_inst_cache;
        logic [15:0] trm_tx_fifo128x32  ; assign trm_tx_fifo128x32  = trmdat[20]; localparam t_trm IV_trm_tx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_rx_fifo128x32  ; assign trm_rx_fifo128x32  = trmdat[21]; localparam t_trm IV_trm_rx_fifo128x32  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_fifo32x19      ; assign trm_fifo32x19      = trmdat[22]; localparam t_trm IV_trm_fifo32x19      = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_share   ; assign trm_udcmem_share   = trmdat[23]; localparam t_trm IV_trm_udcmem_share   = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_odb     ; assign trm_udcmem_odb     = trmdat[24]; localparam t_trm IV_trm_udcmem_odb     = IV_rf_2p_hdc_inst;
        logic [15:0] trm_udcmem_256x64  ; assign trm_udcmem_256x64  = trmdat[25]; localparam t_trm IV_trm_udcmem_256x64  = IV_rf_2p_hdc_inst;
        logic [15:0] trm_acram2kx64     ; assign trm_acram2kx64     = trmdat[26]; localparam t_trm IV_trm_acram2kx64     = IV_sram_sp_uhde_inst_sram0;
        logic [15:0] trm_aoram1kx36     ; assign trm_aoram1kx36     = trmdat[27]; localparam t_trm IV_trm_aoram1kx36     = IV_sram_sp_hde_inst;

             */
        crate::println!("setting 0.9v sramtrm");
        let mut sramtrm = CSR::new(utra::coresub_sramtrm::HW_CORESUB_SRAMTRM_BASE as *mut u32);
        sramtrm.wo(utra::coresub_sramtrm::SFR_CACHE, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_ITCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_DTCM, 0x3);
        sramtrm.wo(utra::coresub_sramtrm::SFR_VEXRAM, 0x1);

        let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
        // bio 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (19 << 16) | 0b011_000_01_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // vex 0.9v settings
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((15 + i) << 16) | 0b001_010_00_0_0_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }

        // sram 0.9v settings
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (0 << 16) | 0b011_000_01_0_1_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (1 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        crate::println!("setting other 0.9v trims");

        // tcm 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (6 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (7 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // ifram 0.9v
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (8 << 16) | 0b010_000_00_0_1_000_1_01);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);

        // sce 0.9V
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (9 << 16) | 0b011_000_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        for i in 0..4 {
            rbist.wo(utra::rbist_wrp::SFRCR_TRM, ((10 + i) << 16) | 0b011_000_01_0_1_000_0_00);
            rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
        }
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, (14 << 16) | 0b001_010_00_0_0_000_0_00);
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
    }
    let perclk = unsafe { init_clock_asic(SYSTEM_CLOCK_FREQUENCY) };

    // test memory
    crate::println!("\ntest memory...");
    let base = 0x6108_5000;
    let mem_range = unsafe {
        core::slice::from_raw_parts_mut(base as *mut u32, (0x6120_0000 - base) / core::mem::size_of::<u32>())
    };
    let mut state = 1;
    for m in mem_range.iter_mut() {
        state = lfsr_next_u32(state);
        *m = state;
    }
    state = 1;
    let mut failures = 0;
    for m in mem_range.iter() {
        state = lfsr_next_u32(state);
        if state != *m {
            failures += 1;
        }
    }
    if failures != 0 {
        crate::println!("fast mem test failed");
    } else {
        crate::println!("fast mem test passed");
    }
    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);

    // setup the static data (hard-coded, because no loader) - necessary for heap
    // see build message about non-zero data. Also need to zero out the BSS region.
    let static_data = unsafe { core::slice::from_raw_parts_mut(0x6108_0000 as *mut u8, 0x14 + 0x24) };
    static_data.fill(0);
    static_data[8] = 1;
    crate::print!("static data {:x}: ", static_data.as_ptr() as usize);
    for sd in static_data.iter() {
        crate::print!("{:x} ", *sd);
    }
    crate::println!("");

    // setup heap alloc
    setup_alloc();

    setup_timer();

    // Rx setup
    let mut udma_uart = setup_rx(perclk);
    irq_setup();
    enable_irq(utra::irqarray5::IRQARRAY5_IRQ);
    udma_uart.write("console up\r\n".as_bytes());
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
    timer.wfo(utra::timer0::RELOAD_RELOAD, (SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
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

/// Delay function that delays a given number of milliseconds.
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

// This function supercedes init_clock_asic() and needs to be back-ported
// into xous-core
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

    // this block might belong at the top, in particular, configuring the dividers prevents stuff
    // from being overclocked when the PLL comes live; but for now we are debugging other stuff
    {
        // Hits a 16:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
        // Resulting in 800:400:200:100:50 MHz assuming 800MHz fclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_0.offset()).write_volatile(0x3f7f); // fclk

        // Hits a 8:8:4:2:1 ratio on fclk:aclk:hclk:iclk:pclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_1.offset()).write_volatile(0x3f7f); // aclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_2.offset()).write_volatile(0x1f3f); // hclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_3.offset()).write_volatile(0x0f1f); // iclk
        daric_cgu.add(utra::sysctrl::SFR_CGUFD_CFGFDCR_0_4_4.offset()).write_volatile(0x070f); // pclk
        // perclk divider - set to divide by 16 off of an 800Mhz base. Only found on NTO.
        // daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
        // perclk divider - set to divide by 8 off of an 800Mhz base. Only found on NTO.
        // TODO: this only works for two clock settings. Broken @ 600. Need to fix this to instead derive
        // what pclk is instead of always reporting 100mhz
        if freq_hz == 800_000_000 {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x07_ff_ff);
        } else {
            daric_cgu.add(utra::sysctrl::SFR_CGUFDPER.offset()).write_volatile(0x03_ff_ff);
        }

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

        // turn off gates
        daric_cgu.add(utra::sysctrl::SFR_ACLKGR.offset()).write_volatile(0x2f);
        daric_cgu.add(utra::sysctrl::SFR_HCLKGR.offset()).write_volatile(0xff);
        daric_cgu.add(utra::sysctrl::SFR_ICLKGR.offset()).write_volatile(0x8f);
        daric_cgu.add(utra::sysctrl::SFR_PCLKGR.offset()).write_volatile(0xff);
        crate::println!("bef gates set");
        for _ in 0..100 {
            crate::print!("*");
        }
        crate::println!(".");
        // commit dividers
        daric_cgu.add(utra::sysctrl::SFR_CGUSET.offset()).write_volatile(0x32);
        crate::println!("gates set");
        for _ in 0..100 {
            crate::print!("-");
        }
        crate::println!(".");
    }

    /*
    if (0 == (cgu.r(sysctrl::SFR_IPCPLLMN) & 0x0001F000))
        || (0 == (cgu.r(sysctrl::SFR_IPCPLLMN) & 0x00000fff))
    {
        // for SIM, avoid div by 0 if unconfigurated
        // , default VCO 48MHz / 48 * 1200 = 1.2GHz
        // TODO magic numbers
        cgu.wo(sysctrl::SFR_IPCPLLMN, ((M << 12) & 0x0001F000) | ((1200) & 0x00000fff));
        cgu.wo(sysctrl::SFR_IPCPLLF, 0);
        cgu.wo(sysctrl::SFR_IPCARIPFLOW, 0x32);
    }
    */
    for _ in 0..100 {
        crate::print!("1");
    }
    crate::println!(".");

    // TODO select int/ext osc/xtal
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

    for _ in 0..100 {
        crate::print!("2");
    }
    crate::println!(".");

    if freq_hz < 1000000 {
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

    for _ in 0..100 {
        crate::print!("3");
    }
    crate::println!(".");

    if freq_hz < 1000000 {
    } else {
        let n_fxp24: u64; // fixed point
        let f16mhz_log2: u32 = (freq_hz / FREQ_0).ilog2();

        for _ in 0..100 {
            crate::print!("4");
        }
        crate::println!(".");

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

        //printf("read reg a0 : %08" PRIx32"\n", *((volatile uint32_t* )0x400400a0));
        //printf("read reg a4 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a4));
        //printf("read reg a8 : %04" PRIx16"\n", *((volatile uint16_t* )0x400400a8));

        // TODO wait/poll lock status?
        // DARIC_CGU->cgusel0 = 1; // clktop sel, 0:clksys, 1:clkpll0
        cgu.wo(sysctrl::SFR_CGUSEL0, 1);
        // __DSB();
        // DARIC_CGU->cguset = 0x32; // commit
        cgu.wo(sysctrl::SFR_CGUSET, 0x32);
        crate::println!("clocks set");

        // __DSB();

        // printf ("    MN: 0x%05x, F: 0x%06x, Q: 0x%04x\n",
        //     DARIC_IPC->pll_mn, DARIC_IPC->pll_f, DARIC_IPC->pll_q);
        // printf ("    LPEN: 0x%01x, OSC: 0x%04x, BIAS: 0x%04x,\n",
        //     DARIC_IPC->lpen, DARIC_IPC->osc, DARIC_IPC->ipc);
    }
    crate::println!(
        "mn {:x}, q{:x}",
        (0x400400a0 as *const u32).read_volatile(),
        (0x400400a8 as *const u32).read_volatile()
    );

    crate::println!("fsvalid: {}", daric_cgu.add(sysctrl::SFR_CGUFSVLD.offset()).read_volatile());
    let cgufsfreq0 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ0.offset()).read_volatile();
    let cgufsfreq1 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ1.offset()).read_volatile();
    let cgufsfreq2 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ2.offset()).read_volatile();
    let cgufsfreq3 = daric_cgu.add(sysctrl::SFR_CGUFSSR_FSFREQ3.offset()).read_volatile();

    crate::println!(
        "Internal osc: {} -> {} MHz ({} MHz)",
        cgufsfreq0,
        fsfreq_to_hz(cgufsfreq0),
        fsfreq_to_hz_32(cgufsfreq0)
    );
    crate::println!(
        "XTAL: {} -> {} MHz ({} MHz)",
        cgufsfreq1,
        fsfreq_to_hz(cgufsfreq1),
        fsfreq_to_hz_32(cgufsfreq1)
    );
    crate::println!(
        "pll output 0: {} -> {} MHz ({} MHz)",
        cgufsfreq2,
        fsfreq_to_hz(cgufsfreq2),
        fsfreq_to_hz_32(cgufsfreq2)
    );
    crate::println!(
        "pll output 1: {} -> {} MHz ({} MHz)",
        cgufsfreq3,
        fsfreq_to_hz(cgufsfreq3),
        fsfreq_to_hz_32(cgufsfreq3)
    );

    crate::println!("PLL configured to {} MHz", freq_hz / 1_000_000);
    100_000_000
}

#[allow(dead_code)]
fn fsfreq_to_hz(fs_freq: u32) -> u32 { (fs_freq * (48_000_000 / 32)) / 1_000_000 }

#[allow(dead_code)]
fn fsfreq_to_hz_32(fs_freq: u32) -> u32 { (fs_freq * (32_000_000 / 32)) / 1_000_000 }

/// used to generate some test vectors
#[allow(dead_code)]
pub fn lfsr_next_u32(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}
