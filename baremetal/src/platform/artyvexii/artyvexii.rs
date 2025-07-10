use utralib::*;

#[global_allocator]
static ALLOCATOR: linked_list_allocator::LockedHeap = linked_list_allocator::LockedHeap::empty();

// RAM size has 2 pages taken off the top to make space for exception handlers
pub const RAM_SIZE: usize = utralib::generated::HW_MAIN_RAM_MEM_LEN - 8192;
pub const RAM_BASE: usize = utralib::generated::HW_MAIN_RAM_MEM;

// scratch page for exceptions located at top of RAM
// NOTE: there is an additional page above this for exception stack
pub const SCRATCH_PAGE: usize = RAM_BASE + RAM_SIZE;

// Arbitrarily placing heap at 16k into RAM, with a 32k length
// Total memory is 400k in size
// RAM starts at 256k, giving 144k RAM total
// Bottom 4k is for static data (.bss section)
// next 64k is heap; rest is stack.
pub const ROM_LEN: usize = 64 * 1024;
pub const BSS_LEN: usize = 4 * 1024;
pub const HEAP_START: usize = RAM_BASE + ROM_LEN + BSS_LEN;
pub const HEAP_LEN: usize = 1024 * 32;

pub const SYSTEM_CLOCK_FREQUENCY: u32 = 40_000_000;
pub const SYSTEM_TICK_INTERVAL_MS: u32 = 1;

pub fn early_init() {
    // setup interrupts & enable IRQ handler for characters
    crate::irq::irq_setup();
    crate::debug::Uart::enable_rx(true);
    crate::irq::enable_irq(utra::uart::UART_IRQ);

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

    setup_alloc();
}

pub fn setup_alloc() {
    // Initialize the allocator with heap memory range
    crate::println!("Setting up heap @ {:x}-{:x}", HEAP_START, HEAP_START + HEAP_LEN);
    crate::println!("Stack @ {:x}-{:x}", HEAP_START + HEAP_LEN, RAM_BASE + RAM_SIZE);
    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_LEN);
    }
}

// Install a panic handler when not running tests.
#[cfg(all(not(test)))]
mod panic_handler {
    use core::panic::PanicInfo;
    #[panic_handler]
    fn handle_panic(_arg: &PanicInfo) -> ! {
        crate::println!("{}", _arg);
        loop {}
    }
}

/// Delay function that delays a given number of milliseconds.
pub fn delay(ms: usize) {
    let mut timer = CSR::new(utra::timer0::HW_TIMER0_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    for _ in 0..ms {
        while timer.rf(utra::timer0::EV_PENDING_ZERO) == 0 {}
        timer.wfo(utra::timer0::EV_PENDING_ZERO, 1);
    }
}
