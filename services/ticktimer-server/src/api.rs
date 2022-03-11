#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// Get the elapsed time in milliseconds
    ElapsedMs = 0,

    /// Sleep for the specified numer of milliseconds
    SleepMs = 1,

    /// Recalculate the sleep time
    RecalculateSleep = 2,

    /// Suspend/resume callback
    SuspendResume = 3,

    /// force a WDT update
    PingWdt = 4,

    /// Return the version string of Xous. We bury it here because this is a small, lightweight server we can rebuild on every run.
    GetVersion = 5,

    /// Lock the given Mutex, blocking if it is already locked.
    /// 
    /// # Arguments
    /// 
    /// *arg1*: An integer of some sort, such as the address of the Mutex
    LockMutex = 6,

    /// Unlock the given Mutex
    /// 
    /// # Arguments
    /// 
    /// *arg1*: An integer of some sort, such as the address of the Mutex
    UnlockMutex = 7,

    /// Wait for a given condition to be signalled
    /// 
    /// # Arguments
    /// 
    /// *arg1*: An integer of some sort, such as the address of the Condvar
    /// *arg2*: The number of milliseconds to wait, or 0 to wait forever
    WaitForCondition = 8,

    /// Notify a condition
    /// 
    /// # Arguments
    /// 
    /// *arg1*: An integer of some sort, such as the address of the Condvar
    /// *arg2*: The number of conditions to notify
    NotifyCondition = 9,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct VersionString {
    pub version: xous_ipc::String::<512>,
}