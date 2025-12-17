use bao1x_api::*;
use bao1x_hal::iox::Iox;
use bao1x_hal::udma;
use utralib::CSR;
use utralib::utra;

use crate::platform::{
    debug::setup_rx,
    irq::{enable_irq, irq_setup},
};

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

pub const RAM_SIZE: usize = utralib::generated::HW_SRAM_MEM_LEN;
pub const RAM_BASE: usize = utralib::generated::HW_SRAM_MEM;
pub const SIGBLOCK_LEN: usize = 768; // this is adjusted inside builder.rs, in the sign-image invocation

const DATA_SIZE_BYTES: usize = 0x6000;
pub const HEAP_START: usize = RAM_BASE + DATA_SIZE_BYTES;
pub const HEAP_LEN: usize = 1024 * 256;

// scratch page for exceptions
//   - scratch data is stored in positive offsets from here
//   - exception stack is stored in negative offsets from here, hence the +4096
// total occupied area is [HEAP_START + HEAP_LEN..HEAP_START + HEAP_LEN + 8192]
pub const SCRATCH_PAGE: usize = HEAP_START + HEAP_LEN + 4096;

pub const UART_IFRAM_ADDR: usize = bao1x_hal::board::UART_DMA_TX_BUF_PHYS;

pub const SYSTEM_CLOCK_FREQUENCY: u32 = 700_000_000;

pub fn early_init() {
    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);

    // ASSUME baosec target.
    // ASSUME boot1:
    //   - sets up all the keep-ons & basic pins

    // Ensure SRAM timings are set for 900mV operation before setting fast clock frequency. We will
    // be running at full tilt on baosec.
    let trim_table =
        bao1x_hal::sram_trim::get_sram_trim_for_voltage(bao1x_api::offsets::dabao::CPU_VDD_LDO_BOOT_MV);
    let mut rbist = CSR::new(utra::rbist_wrp::HW_RBIST_WRP_BASE as *mut u32);
    for item in trim_table {
        rbist.wo(utra::rbist_wrp::SFRCR_TRM, item.raw_value());
        rbist.wo(utra::rbist_wrp::SFRAR_TRM, 0x5a);
    }

    // Now that SRAM trims are setup, initialize all the statics by writing to memory.
    // For baremetal, the statics structure is just at the flash base.
    const STATICS_LOC: usize = bao1x_api::BAREMETAL_START + SIGBLOCK_LEN;

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
            data_ptr
                .add(u16::from_le_bytes(offset) as usize / size_of::<u32>())
                .write_volatile(u32::from_le_bytes(data));
        }
    }

    // set the clock
    let fclk = SYSTEM_CLOCK_FREQUENCY;
    let perclk = unsafe {
        bao1x_hal::clocks::init_clock_asic(
            fclk,
            utra::sysctrl::HW_SYSCTRL_BASE,
            utralib::HW_AO_SYSCTRL_BASE,
            Some(utra::duart::HW_DUART_BASE),
            delay_at_sysfreq,
            true,
        )
    };
    // setup heap alloc
    setup_alloc();

    setup_timer();

    // Rx setup
    let mut udma_uart = setup_rx(perclk);
    irq_setup();
    enable_irq(utra::irqarray5::IRQARRAY5_IRQ);

    udma_uart.write("baremetal console up\r\n".as_bytes());
    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);

    // Baosec specific:
    // Setup I/Os so things that should be powered off are actually off
    #[cfg(feature = "board-baosec")]
    {
        bao1x_hal::board::setup_display_pins(&iox);
        bao1x_hal::board::setup_memory_pins(&iox);
        bao1x_hal::board::setup_i2c_pins(&iox);
        bao1x_hal::board::setup_camera_pins(&iox);
        bao1x_hal::board::setup_kb_pins(&iox);
        bao1x_hal::board::setup_oled_power_pin(&iox);

        let trng_power = bao1x_hal::board::setup_trng_power_pin(&iox);
        // kernel expects the TRNG to be on
        iox.set_gpio_pin(trng_power.0, trng_power.1, bao1x_api::IoxValue::High);

        let (port, pin) = bao1x_hal::board::setup_dcdc2_pin(&iox);
        // low connects DCDC2 to the chip
        iox.set_gpio_pin(port, pin, IoxValue::Low);

        // make sure SE0 is cleared
        let (port, pin) = bao1x_hal::board::setup_usb_pins(&iox);
        iox.set_gpio_pin(port, pin, IoxValue::High);
    }
    #[cfg(feature = "board-dabao")]
    {
        // this actively drives the pin high, allowing USB to connect
        bao1x_hal::board::setup_usb_pins(&iox);
        // this puts the pin into a tri-state
        bao1x_hal::board::setup_boot_pin(&iox);
    }
    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);
    crate::println!("CPU freq: {} MHz", SYSTEM_CLOCK_FREQUENCY / 2);
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

/// used to generate some test vectors
#[allow(dead_code)]
pub fn lfsr_next_u32(state: u32) -> u32 {
    let bit = ((state >> 31) ^ (state >> 21) ^ (state >> 1) ^ (state >> 0)) & 1;

    (state << 1) + bit
}

pub fn clockset_wrapper(freq: u32) -> u32 {
    // reset the baud rate on the console UART
    let perclk = unsafe {
        bao1x_hal::clocks::init_clock_asic(
            freq,
            utra::sysctrl::HW_SYSCTRL_BASE,
            utralib::HW_AO_SYSCTRL_BASE,
            Some(utra::duart::HW_DUART_BASE),
            delay_at_sysfreq,
            true,
        )
    };
    let uart_buf_addr = crate::platform::UART_IFRAM_ADDR;
    #[cfg(feature = "bao1x-evb")]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_1::HW_UDMA_UART_1_BASE, uart_buf_addr, uart_buf_addr)
    };
    #[cfg(not(feature = "bao1x-evb"))]
    let mut udma_uart = unsafe {
        // safety: this is safe to call, because we set up clock and events prior to calling
        // new.
        udma::Uart::get_handle(utra::udma_uart_2::HW_UDMA_UART_2_BASE, uart_buf_addr, uart_buf_addr)
    };
    let baudrate: u32 = crate::UART_BAUD;
    let freq: u32 = perclk;
    udma_uart.set_baud(baudrate, freq);

    crate::println!("clock set done, perclk is {} MHz", perclk / 1_000_000);
    udma_uart.write("console up with clocks\r\n".as_bytes());

    perclk
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KeyPress {
    Up,
    Down,
    Left,
    Right,
    Select,
    Home,
    Invalid,
    None,
}
#[allow(dead_code)]
pub fn scan_keyboard<T: IoSetup + IoGpio>(
    iox: &T,
    rows: &[(IoxPort, u8)],
    cols: &[(IoxPort, u8)],
) -> [KeyPress; 4] {
    let mut key_presses: [KeyPress; 4] = [KeyPress::None; 4];
    let mut key_press_index = 0; // no Vec in no_std, so we have to manually track it

    for (row, (port, pin)) in rows.iter().enumerate() {
        iox.set_gpio_pin_value(*port, *pin, IoxValue::Low);
        for (col, (col_port, col_pin)) in cols.iter().enumerate() {
            if iox.get_gpio_pin_value(*col_port, *col_pin) == IoxValue::Low {
                crate::println!("Key press at ({}, {})", row, col);
                if key_press_index < key_presses.len() {
                    key_presses[key_press_index] = match (row, col) {
                        (1, 3) => KeyPress::Left,
                        (1, 2) => KeyPress::Home,
                        (1, 0) => KeyPress::Right,
                        (0, 0) => KeyPress::Down,
                        (0, 2) => KeyPress::Up,
                        (0, 1) => KeyPress::Select,
                        _ => KeyPress::Invalid,
                    };
                    key_press_index += 1;
                }
            }
        }
        iox.set_gpio_pin_value(*port, *pin, IoxValue::High);
    }
    key_presses
}

#[derive(Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
#[cfg(feature = "trng-debug")]
#[repr(u32)]
pub enum TrngOpt {
    RngA = 0x0,
    RngB = 0x1_0000,
}

#[cfg(feature = "trng-debug")]
fn trng_stop(trng: &mut CSR<u32>, sce: &mut CSR<u32>, opt: TrngOpt) {
    trng.wo(utra::trng::SFR_AR_GEN, 0xa5); // trigger stop
    match opt {
        TrngOpt::RngB => {
            sce.wo(utra::sce_glbsfr::SFR_FFEN, !(1 << 5)); // disable fifo
        }
        TrngOpt::RngA => {
            sce.wo(utra::sce_glbsfr::SFR_FFEN, !(1 << 4)); // disable fifo
        }
    }
}

#[cfg(feature = "trng-debug")]
fn trng_raw_buf_ready(trng: &CSR<u32>) -> bool { trng.r(utra::trng::SFR_SR) & (1 << 28) != 0 }

#[allow(dead_code)]
#[cfg(feature = "trng-debug")]
pub fn trng_ro(
    crsrc: u32,
    crana: u32,
    post_proc: u32,
    opt: TrngOpt,
    chain0_l: u32,
    chain0_h: u32,
    chain1_l: u32,
    chain1_h: u32,
    buf: &mut [u32],
    do_raw: bool,
) {
    let mut trng = CSR::new(utralib::utra::trng::HW_TRNG_BASE as *mut u32);
    let mut sce = CSR::new(utralib::utra::sce_glbsfr::HW_SCE_GLBSFR_BASE as *mut u32);
    let rngb_mem = unsafe {
        core::slice::from_raw_parts(
            utralib::HW_SEG_RNGB_MEM as *const u32,
            utralib::HW_SEG_RNGB_MEM_LEN / size_of::<u32>(),
        )
    };
    let rnga_mem = unsafe {
        core::slice::from_raw_parts(
            utralib::HW_SEG_RNGA_MEM as *const u32,
            utralib::HW_SEG_RNGA_MEM_LEN / size_of::<u32>(),
        )
    };

    match opt {
        TrngOpt::RngB => sce.wo(utra::sce_glbsfr::SFR_FFEN, 1 << 5),
        TrngOpt::RngA => sce.wo(utra::sce_glbsfr::SFR_FFEN, 1 << 4),
    }

    trng.wo(utra::trng::SFR_CRSRC, crsrc);
    trng.wo(utra::trng::SFR_CRANA, crana);
    trng.wo(utra::trng::SFR_PP, post_proc);
    trng.wo(utra::trng::SFR_OPT, opt as u32 | 0x100);
    // contex.trng->opt = 0x10040;   // use rngB , gen 0x40*4*4=1024 bytes 为啥现在是 0x100 *4 ???
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN0, chain0_l);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN1, chain0_h);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN2, chain1_l);
    trng.wo(utra::trng::SFR_CHAIN_RNGCHAINEN3, chain1_h);

    /*
    crate::println!("crsrc: {:08x}", trng.r(utra::trng::SFR_CRSRC));
    crate::println!("crana: {:08x}", trng.r(utra::trng::SFR_CRANA));
    crate::println!("postproc: {:08x}", trng.r(utra::trng::SFR_PP));
    crate::println!("opt: {:08x}", trng.r(utra::trng::SFR_OPT));
    crate::println!("chain0_l: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN0));
    crate::println!("chain0_h: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN1));
    crate::println!("chain1_l: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN2));
    crate::println!("chain1_h: {:08x}", trng.r(utra::trng::SFR_CHAIN_RNGCHAINEN3));

    crate::println!("rng start");
    */
    if !do_raw {
        for (genloop, chunk) in buf.chunks_mut(256).enumerate() {
            match opt {
                TrngOpt::RngB => sce.wo(utra::sce_glbsfr::SFR_FFCLR, 0x0000FF05),
                TrngOpt::RngA => sce.wo(utra::sce_glbsfr::SFR_FFCLR, 0x0000FF04),
            }

            if genloop != 0 {
                /*
                crate::println!(
                    "GEN_LOOP = {},HLTHTEST_ERRCNT (After gen)= {}",
                    genloop - 1,
                    ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                ); //上一次结果打印
                */
            }
            /*
            crate::println!(
                "GEN_LOOP = {} ,HLTHTEST_ERRCNT (Before gen)= {}",
                genloop,
                ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
            ); //新的起始打印
            */
            trng.wo(utra::trng::SFR_AR_GEN, 0x5a); // trigger start

            match opt {
                TrngOpt::RngB => {
                    while (sce.r(utra::sce_glbsfr::SFR_FFCNT_SR_FF5) >> 4) & 0xFFF < chunk.len() as u32 {
                        // wait
                    }
                    chunk.copy_from_slice(&rngb_mem[..chunk.len()]);
                    /*
                    crate::println!(
                        "GEN_LOOP = {} ,HLTHTEST_ERRCNT (After gen)= {}\r\n",
                        genloop,
                        ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                    );
                    */
                }
                TrngOpt::RngA => {
                    while (sce.r(utra::sce_glbsfr::SFR_FFCNT_SR_FF4) >> 4) & 0xFFF < chunk.len() as u32 {
                        // wait
                    }
                    chunk.copy_from_slice(&rnga_mem[..chunk.len()]);
                    /*
                    crate::println!(
                        "GEN_LOOP = {} ,HLTHTEST_ERRCNT (After gen)= {}\r\n",
                        genloop,
                        ((trng.r(utra::trng::SFR_SR) >> 16) & 0x00FF)
                    );
                    */
                }
            }
        }
        // crate::println!("HLTHTEST_ERRCNT (After gen)= {}\r\n", ((trng.r(utra::trng::SFR_SR) >> 16) &
        // 0x00FF));
        trng_stop(&mut trng, &mut sce, opt);
    } else {
        // make sure the trng is in the stopped state
        trng_stop(&mut trng, &mut sce, opt);
        // crate::println!("raw data");

        for d in buf.iter_mut() {
            while !trng_raw_buf_ready(&trng) {} // wait for buffer to fill
            *d = trng.r(utra::trng::SFR_BUF);
        }
    }
    // crate::println!("rng stop");
}
