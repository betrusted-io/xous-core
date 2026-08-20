use bao1x_api::*;
use bao1x_hal::hardening::*;
use bao1x_hal::iox::Iox;
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

pub fn early_init() -> Csprng {
    let mut csprng = Csprng::new();
    csprng.random_delay();

    // Ensure SRAM timings are set for 900mV operation before setting fast clock frequency. We will
    // be running at full tilt on baosec.
    let trim_table =
        bao1x_hal::sram_trim::get_sram_trim_for_voltage(bao1x_api::offsets::baosec::CPU_VDD_LDO_BOOT_MV);
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

    csprng.random_delay();

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

    csprng.random_delay();

    // setup heap alloc
    setup_alloc();

    setup_timer();

    irq_setup();

    let iox = Iox::new(utra::iox::HW_IOX_BASE as *mut u32);
    let power_off_port = bao1x_hal::board::get_power_off_pin();
    iox.setup_pin(
        power_off_port.0,
        power_off_port.1,
        Some(IoxDir::Output),
        Some(IoxFunction::Gpio),
        None,
        Some(IoxEnable::Disable),
        None,
        Some(IoxDriveStrength::Drive2mA),
    );
    // ensures the device stays on
    iox.set_gpio_pin(power_off_port.0, power_off_port.1, IoxValue::Low);

    let (se0_port, se0_pin) = bao1x_hal::board::setup_usb_pins(&iox);
    iox.set_gpio_pin(se0_port, se0_pin, IoxValue::Low); // put the USB port into SE0 while we initialize things

    // setup display - turn on its power, reset the framebuffer
    // reset enable - note inversion compared to baosec (at least as of this board rev of baosec)
    bao1x_hal::board::setup_periph_reset_pin(&iox);

    bao1x_hal::board::assert_periph_reset(&iox, true);
    // delay while reset assert - setup display pins at this time
    bao1x_hal::board::setup_display_pins(&iox);
    let (oled_on_port, oled_on_pin) = bao1x_hal::board::setup_oled_power_pin(&iox);
    bao1x_hal::board::assert_periph_reset(&iox, false);

    // power on
    iox.set_gpio_pin_value(oled_on_port, oled_on_pin, IoxValue::High);

    // Rx setup
    let mut udma_uart = setup_rx(perclk);
    enable_irq(utra::irqarray5::IRQARRAY5_IRQ);
    crate::debug::USE_CONSOLE.store(true, core::sync::atomic::Ordering::SeqCst);

    crate::println!("scratch page: {:x}, heap start: {:x}", SCRATCH_PAGE, HEAP_START);
    crate::println!("CPU freq: {} MHz", SYSTEM_CLOCK_FREQUENCY / 2);
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
