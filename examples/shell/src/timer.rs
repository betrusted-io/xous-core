
#[cfg(baremetal)]
const SYSTEM_CLOCK_FREQUENCY: u32 = 12_000_000;
#[cfg(baremetal)]
const TIMER_BASE: usize = 0xF000_3000;

#[cfg(baremetal)]
fn timer_tick(_irq_no: usize, _arg: *mut usize) {
    println!(">>> Timer tick");
    let ptr = TIMER_BASE as *mut usize;

    xous::rsyscall(xous::SysCall::ReturnToParentI(0, 0)).expect("couldn't return to parent");

    // acknowledge the timer
    unsafe { ptr.add(6).write_volatile(1) };
    println!("<<< Returning from timer_tick()");
}

#[cfg(baremetal)]
pub fn init() {
    use xous::{MemoryAddress, MemorySize};
    println!("Allocating timer...");
    xous::rsyscall(xous::SysCall::MapMemory(
        MemoryAddress::new(TIMER_BASE),
        MemoryAddress::new(TIMER_BASE),
        MemorySize::new(4096).unwrap(),
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ))
    .expect("timer: couldn't map timer");

    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        1,
        MemoryAddress::new(timer_tick as *mut usize as usize).unwrap(),
        None,
    ))
    .expect("timer: couldn't claim interrupt");

    let ms = 10; // tick every 10 ms
    en(false);
    load(SYSTEM_CLOCK_FREQUENCY / 1_000 * ms);
    reload(SYSTEM_CLOCK_FREQUENCY / 1_000 * ms);
    en(true);

    // Set EV_ENABLE
    let ptr = TIMER_BASE as *mut usize;
    unsafe { ptr.add(7).write_volatile(1) };
}

#[cfg(not(baremetal))]
pub fn init() {}

// pub fn load(value: u32) {
//     let ptr = TIMER_BASE as *mut usize;
//     unsafe {
//         ptr.add(0).write_volatile(value as usize);
//     }
// }

// pub fn reload(value: u32) {
//     let ptr = TIMER_BASE as *mut usize;
//     unsafe {
//         ptr.add(1).write_volatile(value as usize);
//     }
// }

// pub fn en(en: bool) {
//     let ptr = TIMER_BASE as *mut usize;
//     if en {
//         unsafe { ptr.add(2).write_volatile(1) };
//     } else {
//         unsafe { ptr.add(2).write_volatile(0) };
//     }
// }
