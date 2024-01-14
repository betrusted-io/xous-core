pub enum AtomicOperation {
    /// Lock the mutex at the given address. If the Mutex is locked, this
    /// thread will block.
    MutexLock(usize /* Address */),

    /// Unlock the mutex at the given address. If arg 2 is `true`, then
    /// the next thread that is waiting on this Mutex will be activated.
    /// This can be used to prevent deadlocks. If no thread is waiting,
    /// then no thread switch is made and execution resumes in the current
    /// thread.
    MutexUnlock(usize /* Address */, bool /* Should Switch Immediately */),


    CondvarWait(usize /* Address */),
    CondvarWaitTimeout(usize /* Address */, usize /* Timeout (ms) */),
    CondvarNotifyOne(usize /* Address */),
    CondvarNotifyAll(usize /* Address */),
}