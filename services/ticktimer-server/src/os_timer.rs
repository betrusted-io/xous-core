#[cfg(baremetal)]
use utralib::generated::*;

#[cfg(baremetal)]
const SYSTEM_CLOCK_FREQUENCY: u32 = 100_000_000;
#[cfg(baremetal)]
const TIMER_BASE: usize = utra::timer0::HW_TIMER0_BASE; // claim the same address in virt as in phys

#[cfg(baremetal)]
fn timer_tick(_irq_no: usize, _arg: *mut usize) {
    //println!(">>> Timer tick");
    let mut timer = CSR::new(TIMER_BASE as *mut u32);

    xous::rsyscall(xous::SysCall::ReturnToParent(xous::PID::new(1).unwrap(), 0))
        .expect("couldn't return to parent");

    // acknowledge the timer
    timer.wfo(utra::timer0::EV_PENDING_ZERO, 0b1);
    //println!("<<< Returning from timer_tick()");
}

#[cfg(baremetal)]
pub fn init() {
    use xous::{MemoryAddress, MemorySize};
    println!("Allocating timer...");
    xous::rsyscall(xous::SysCall::MapMemory(
        MemoryAddress::new(utra::timer0::HW_TIMER0_BASE),
        MemoryAddress::new(TIMER_BASE),
        MemorySize::new(4096).unwrap(),
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ))
    .expect("timer: couldn't map timer");

    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        utra::timer0::TIMER0_IRQ,
        MemoryAddress::new(timer_tick as *mut usize as usize).unwrap(),
        None,
    ))
    .expect("timer: couldn't claim interrupt");

    let ms = 100; // tick every 100 ms
    en(false);
    load((SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
    reload((SYSTEM_CLOCK_FREQUENCY / 1_000) * ms);
    en(true);

    // Set EV_ENABLE
    let mut timer = CSR::new(TIMER_BASE as *mut u32);
    timer.wfo(utra::timer0::EV_ENABLE_ZERO, 0b1);
}

#[cfg(not(baremetal))]
pub fn init() {}

#[cfg(baremetal)]
pub fn load(value: u32) {
    let mut timer = CSR::new(TIMER_BASE as *mut u32);
    timer.wfo(utra::timer0::LOAD_LOAD, value);
}

#[cfg(baremetal)]
pub fn reload(value: u32) {
    let mut timer = CSR::new(TIMER_BASE as *mut u32);
    timer.wfo(utra::timer0::RELOAD_RELOAD, value);
}

#[cfg(baremetal)]
pub fn en(en: bool) {
    let mut timer = CSR::new(TIMER_BASE as *mut u32);
    if en {
        timer.wfo(utra::timer0::EN_EN, 0b1);
    } else {
        timer.wfo(utra::timer0::EN_EN, 0b0);
    }
}
