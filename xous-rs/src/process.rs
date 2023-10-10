use core::sync::atomic::{AtomicU32, Ordering};

static CACHED_PID: AtomicU32 = AtomicU32::new(0);

pub fn id() -> u32 {
    let pid = CACHED_PID.load(Ordering::Relaxed);
    if pid != 0 {
        return pid;
    }

    if let Ok(pid) = crate::syscall::current_pid() {
        CACHED_PID.store(pid.get() as u32, Ordering::Relaxed);
        pid.get() as u32;
    } else {
        0
    }
}

pub fn set_id(id: u32) {
    CACHED_PID.store(id, Ordering::Relaxed);
}
