# Xous API: ticktimer

The Xous `ticktimer` helps other processes track the passage of time through several mechanisms:

- It can report the elapsed uptime since boot in milliseconds.
- It can block a process for a specified number of milliseconds.
- It can block a process until a condition is met (i.e., condvar)

Processes that are blocked by `ticktimer` are entirely de-scheduled and consume no CPU
quantum; the only overhead is a few instructions to check the processes' runnability
state in the kernel's simple round-robin thread scheduler. Thus `sleep` and `condvar`
blocking states are very efficient.

`ticktimer`'s perception of time stops when a system goes into the suspend state;
thus on resume, the elapsed time picks up exactly where it left off. Wall-clock time
during suspend is tracked by the RTC module.

Xous currently has no notions of thread priority, but if it were to develop one,
the `ticktimer` would be the logical place to implement such a feature, as it has
a full view of all the waiting and runnable threads, and it can influence which ones
should be unblocked at a given quantum.
