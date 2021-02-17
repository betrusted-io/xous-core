#[cfg(not(target_os = "none"))]
pub fn id() -> u32 {
    std::process::id()
}

#[cfg(target_os = "none")]
static PID_KEEPER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[cfg(target_os = "none")]
pub fn id() -> u32 {
    PID_KEEPER.load(core::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_os = "none")]
pub fn set_id(id: u32) {
    PID_KEEPER.store(id, core::sync::atomic::Ordering::Relaxed)
}
