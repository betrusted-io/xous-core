pub fn id() -> u32 {
    if let Ok(pid) = crate::syscall::current_pid() {
        pid.get() as u32
    } else {
        0
    }
}

pub fn set_id(_id: u32) {}
