const SYSTEM_CLOCK_FREQUENCY: u32 = 12_000_000;
const TIMER_BASE: usize = 0xF000_3000;

fn timer_tick(_irq_no: usize, _arg: *mut usize) {
    println!("Timer tick");
    let ptr = TIMER_BASE as *mut usize;

    // acknowledge the timer
    unsafe { ptr.add(15).write_volatile(1) };
}

pub fn init() {
    println!("Allocating timer...");
    xous::rsyscall(xous::SysCall::MapPhysical(
        TIMER_BASE as *mut usize,
        TIMER_BASE as *mut usize,
        4096,
        xous::MemoryFlags::R | xous::MemoryFlags::W,
    ))
    .expect("timer: couldn't map timer");

    xous::rsyscall(xous::SysCall::ClaimInterrupt(
        1,
        timer_tick as *mut usize,
        0 as *mut usize,
    ))
    .expect("timer: couldn't claim interrupt");

    let ms = 1000; // tick every 1000 ms
    en(false);
    load(SYSTEM_CLOCK_FREQUENCY / 1_000 * ms);
    reload(SYSTEM_CLOCK_FREQUENCY / 1_000 * ms);
    en(true);

    // Set EV_ENABLE
    let ptr = TIMER_BASE as *mut usize;
    unsafe { ptr.add(16).write_volatile(1) };
}

pub fn load(value: u32) {
    let buf = value.to_le_bytes();
    let ptr = TIMER_BASE as *mut usize;
    unsafe {
        ptr.add(0).write_volatile(buf[0] as usize);
        ptr.add(1).write_volatile(buf[1] as usize);
        ptr.add(2).write_volatile(buf[2] as usize);
        ptr.add(3).write_volatile(buf[3] as usize);
    }
}

pub fn reload(value: u32) {
    let buf = value.to_le_bytes();
    let ptr = TIMER_BASE as *mut usize;
    unsafe {
        ptr.add(4).write_volatile(buf[0] as usize);
        ptr.add(5).write_volatile(buf[1] as usize);
        ptr.add(6).write_volatile(buf[2] as usize);
        ptr.add(7).write_volatile(buf[3] as usize);
    }
}

pub fn en(en: bool) {
    let ptr = TIMER_BASE as *mut usize;
    if en {
        unsafe { ptr.add(8).write_volatile(1) };
    }
    else {
        unsafe { ptr.add(8).write_volatile(0) };
    }
}
