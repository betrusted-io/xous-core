use crate::arch;
use crate::arch::mem::MemoryMapping;
pub use crate::arch::process::Process as ArchProcess;
pub use crate::arch::process::Thread;
use xous_kernel::MemoryRange;

use core::num::NonZeroU8;

use crate::filled_array;
use crate::server::Server;
// use core::mem;
use xous_kernel::{
    pid_from_usize, Error, MemoryAddress, Message, ProcessInit, ThreadInit, CID, PID, SID, TID,
};

const MAX_SERVER_COUNT: usize = 32;

pub use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT};

/// A big unifying struct containing all of the system state.
/// This is inherited from the stage 1 bootloader.
pub struct SystemServices {
    /// A table of all processes in the system
    pub processes: [Process; MAX_PROCESS_COUNT],

    /// A table of all servers in the system
    servers: [Option<Server>; MAX_SERVER_COUNT],

    /// A log of the currently-active syscall depth
    _syscall_stack: [(usize, usize); 3],

    /// How many entries there are on the syscall stack
    _syscall_depth: usize,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ProcessState {
    /// This is an unallocated, free process
    Free,

    /// This process has been allocated, but has no threads yet
    Allocated,

    /// This is a brand-new process that hasn't been run yet, and needs its
    /// initial context set up.
    #[allow(dead_code)]
    Setup(ThreadInit),

    /// This process is able to be run.  The context bitmask describes contexts
    /// that are ready.
    Ready(usize /* context bitmask */),

    /// This is the current active process.  The context bitmask describes
    /// contexts that are ready, excluding the currently-executing context.
    Running(usize /* context bitmask */),

    /// This process is waiting for an event, such as as message or an
    /// interrupt.  There are no contexts that can be run.
    Sleeping,
}

impl Default for ProcessState {
    fn default() -> ProcessState {
        ProcessState::Free
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct Process {
    /// The absolute MMU address.  If 0, then this process is free.  This needs
    /// to be available so we can switch to this process at any time, so it
    /// cannot go into the "inner" struct.
    pub mapping: MemoryMapping,

    /// Where this process is in terms of lifecycle
    state: ProcessState,

    /// This process' PID. This should match up with the index in the process table.
    pub pid: PID,

    /// The process that created this process, which tells who is allowed to
    /// manipulate this process.
    pub ppid: PID,

    /// The current thread ID
    current_thread: TID,

    /// The context number that was active before this process was switched
    /// away.
    previous_thread: TID,
}

impl Default for Process {
    fn default() -> Self {
        Process {
            ppid: unsafe { PID::new_unchecked(1) },
            ..Default::default()
        }
    }
}

/// This is per-process data.  The arch-specific definitions will instantiate
/// this struct in order to avoid the need to statically-allocate this for
/// all possible processes.
/// Note that this data is only available when the current process is active.
#[repr(C)]
#[derive(Debug, PartialEq, Copy, Clone)]
/// Default virtual address when MapMemory is called with no `virt`
pub struct ProcessInner {
    pub mem_default_base: usize,

    /// The last address allocated from
    pub mem_default_last: usize,

    /// Address where messages are passed into
    pub mem_message_base: usize,

    /// The last address that was allocated from
    pub mem_message_last: usize,

    /// Base address of the heap
    pub mem_heap_base: usize,

    /// Current size of the heap
    pub mem_heap_size: usize,

    /// Maximum size of the heap
    pub mem_heap_max: usize,

    /// A mapping of connection IDs to server indexes
    pub connection_map: [Option<NonZeroU8>; 32],

    /// A copy of this process' ID
    pub pid: PID,

    /// Some reserved data to pad this out to a multiple of 32 bytes.
    pub _reserved: [u8; 1],
}

impl Default for ProcessInner {
    fn default() -> Self {
        ProcessInner {
            mem_default_base: arch::mem::DEFAULT_BASE,
            mem_default_last: arch::mem::DEFAULT_BASE,
            mem_message_base: arch::mem::DEFAULT_MESSAGE_BASE,
            mem_message_last: arch::mem::DEFAULT_MESSAGE_BASE,
            mem_heap_base: arch::mem::DEFAULT_HEAP_BASE,
            mem_heap_size: 0,
            mem_heap_max: 524_288,
            connection_map: [None; 32],
            pid: unsafe { PID::new_unchecked(1) },
            _reserved: [0; 1],
        }
    }
}

impl Process {
    /// This process has at least one context that may be run
    pub fn runnable(&self) -> bool {
        match self.state {
            ProcessState::Setup(_) | ProcessState::Ready(_) => true,
            _ => false,
        }
    }

    /// This process slot is unallocated and may be turn into a process
    pub fn free(&self) -> bool {
        match self.state {
            ProcessState::Free => true,
            _ => false,
        }
    }

    pub fn activate(&self) -> Result<(), xous_kernel::Error> {
        crate::arch::process::set_current_pid(self.pid);
        self.mapping.activate()?;
        let mut current_process = crate::arch::process::Process::current();
        current_process.activate()
    }

    pub fn terminate(&mut self) -> Result<(), xous_kernel::Error> {
        if self.free() {
            return Err(xous_kernel::Error::ProcessNotFound);
        }

        // TODO: Free all pages

        // TODO: Free all IRQs

        // TODO: Free memory mapping
        crate::arch::process::Process::destroy(self.pid)?;
        self.state = ProcessState::Free;
        Ok(())
    }
}

#[cfg(not(baremetal))]
std::thread_local!(static SYSTEM_SERVICES: core::cell::RefCell<SystemServices> = core::cell::RefCell::new(SystemServices {
    processes: [Process {
        state: ProcessState::Free,
        ppid: unsafe { PID::new_unchecked(1) },
        pid: unsafe { PID::new_unchecked(1) },
        mapping: arch::mem::DEFAULT_MEMORY_MAPPING,
        current_thread: 0 as TID,
        previous_thread: INITIAL_TID as TID,
    }; MAX_PROCESS_COUNT],
    // Note we can't use MAX_SERVER_COUNT here because of how Rust's
    // macro tokenization works
    servers: filled_array![None; 32],
    _syscall_stack: [(0, 0), (0, 0), (0, 0)],
    _syscall_depth: 0,
}));

#[cfg(baremetal)]
static mut SYSTEM_SERVICES: SystemServices = SystemServices {
    processes: [Process {
        state: ProcessState::Free,
        ppid: unsafe { PID::new_unchecked(1) },
        pid: unsafe { PID::new_unchecked(1) },
        mapping: arch::mem::DEFAULT_MEMORY_MAPPING,
        current_thread: 0 as TID,
        previous_thread: INITIAL_TID as TID,
    }; MAX_PROCESS_COUNT],
    // Note we can't use MAX_SERVER_COUNT here because of how Rust's
    // macro tokenization works
    servers: filled_array![None; 32],
    _syscall_stack: [(0, 0), (0, 0), (0, 0)],
    _syscall_depth: 0,
};

impl core::fmt::Debug for Process {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "Process {} state: {:?}  Memory mapping: {:?}",
            self.pid.get(),
            self.state,
            self.mapping
        )
    }
}

impl SystemServices {
    /// Calls the provided function with the current inner process state.
    pub fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&SystemServices) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&SYSTEM_SERVICES)
        }
        #[cfg(not(baremetal))]
        SYSTEM_SERVICES.with(|ss| f(&ss.borrow()))
    }

    pub fn with_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut SystemServices) -> R,
    {
        #[cfg(baremetal)]
        unsafe {
            f(&mut SYSTEM_SERVICES)
        }

        #[cfg(not(baremetal))]
        SYSTEM_SERVICES.with(|ss| f(&mut ss.borrow_mut()))
    }

    /// Create a new "System Services" object based on the arguments from the
    /// kernel. These arguments decide where the memory spaces are located, as
    /// well as where the stack and program counter should initially go.
    #[cfg(baremetal)]
    pub fn init_from_memory(&mut self, base: *const u32, args: &crate::args::KernelArguments) {
        // Look through the kernel arguments and create a new process for each.
        let init_offsets = {
            let mut init_count = 1;
            for arg in args.iter() {
                if arg.name == make_type!("IniE") {
                    init_count += 1;
                }
            }
            unsafe {
                core::slice::from_raw_parts(
                    base as *const crate::arch::process::InitialProcess,
                    init_count,
                )
            }
        };

        // Copy over the initial process list.  The pid is encoded in the SATP
        // value from the bootloader.  For each process, translate it from a raw
        // KernelArguments value to a SystemServices Process value.
        for init in init_offsets.iter() {
            let pid = (init.satp >> 22) & ((1 << 9) - 1);
            let ref mut process = self.processes[(pid - 1) as usize];
            // println!(
            //     "Process: SATP: {:08x}  PID: {}  Memory: {:08x}  PC: {:08x}  SP: {:08x}  Index: {}",
            //     init.satp,
            //     pid,
            //     init.satp << 10,
            //     init.entrypoint,
            //     init.sp,
            //     pid - 1
            // );
            unsafe {
                process.mapping.from_raw(init.satp);
                process.ppid = PID::new_unchecked(1);
                process.pid = PID::new(pid as _).unwrap();
            };
            if pid == 1 {
                process.state = ProcessState::Running(0);
            } else {
                process.state = ProcessState::Setup(ThreadInit::new(
                    unsafe { core::mem::transmute::<usize, _>(init.entrypoint) },
                    MemoryRange::new(init.sp, crate::arch::process::DEFAULT_STACK_SIZE).unwrap(),
                    MemoryAddress::new(pid),
                    [0u8; 12],
                ));
            }
        }

        // Set up our handle with a bogus sp and pc.  These will get updated
        // once a context switch _away_ from the kernel occurs, however we need
        // to make sure other fields such as "thread number" are all valid.
        ArchProcess::setup_process(PID::new(1).unwrap(), ThreadInit::default())
            .expect("couldn't setup process");
    }

    /// Add a new entry to the process table. This results in a new address space
    /// and a new PID, though the process is in the state `Setup()`.
    pub fn create_process(&mut self, init_process: ProcessInit) -> Result<PID, xous_kernel::Error> {
        for (idx, mut entry) in self.processes.iter_mut().enumerate() {
            if entry.state != ProcessState::Free {
                continue;
            }
            let new_pid = pid_from_usize(idx + 1)?;
            arch::process::Process::create(new_pid, init_process);
            let ppid = crate::arch::process::current_pid();
            // println!("Creating new process for PID {} with PPID {}", new_pid, ppid);
            entry.state = ProcessState::Allocated;
            entry.ppid = ppid;
            entry.pid = new_pid;
            return Ok(new_pid);
        }
        Err(xous_kernel::Error::ProcessNotFound)
    }

    pub fn get_process(&self, pid: PID) -> Result<&Process, xous_kernel::Error> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if cfg!(baremetal) && self.processes[pid_idx].mapping.get_pid() != pid {
            println!(
                "Process doesn't match ({} vs {})",
                self.processes[pid_idx].mapping.get_pid(),
                pid
            );
            return Err(xous_kernel::Error::ProcessNotFound);
        }
        Ok(&self.processes[pid_idx])
    }

    pub fn get_process_mut(&mut self, pid: PID) -> Result<&mut Process, xous_kernel::Error> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;

        // if self.processes[pid_idx].mapping.get_pid() != pid {
        //     println!(
        //         "Process doesn't match ({} vs {})",
        //         self.processes[pid_idx].mapping.get_pid(),
        //         pid
        //     );
        //     return Err(xous_kernel::Error::ProcessNotFound);
        // }
        Ok(&mut self.processes[pid_idx])
    }

    // pub fn current_thread(&self, pid: PID) -> usize {
    //     self.processes[pid.get() as usize - 1].current_thread as usize
    // }

    pub fn current_pid(&self) -> PID {
        arch::process::current_pid()
        // PID0 doesn't exist -- process IDs are offset by 1.
        // assert_eq!(
        //     self.processes[pid as usize - 1].mapping,
        //     MemoryMapping::current(),
        //     "process memory map doesn't match -- current_pid: {}",
        //     pid
        // );
        // assert_eq!(
        //     pid, self.pid,
        //     "current pid {} doesn't match arch pid: {}",
        //     self.pid, pid
        // );
        // pid
    }

    /// Create a stack frame in the specified process and jump to it.
    /// 1. Pause the current process and switch to the new one
    /// 2. Save the process state, if it hasn't already been saved
    /// 3. Run the new process, returning to an illegal instruction
    #[cfg(baremetal)]
    pub fn finish_callback_and_resume(
        &mut self,
        pid: PID,
        tid: TID,
    ) -> Result<(), xous_kernel::Error> {
        // Get the current process (which was the interrupt handler) and mark it
        // as Ready.  Note that the new PID may very well be the same PID.
        {
            let current_pid = self.current_pid();
            let mut current = self
                .get_process_mut(current_pid)
                .expect("couldn't get current PID");
            // println!("Finishing callback in PID {}", current_pid);
            current.state = match current.state {
                ProcessState::Running(0) => ProcessState::Sleeping,
                ProcessState::Running(x) => ProcessState::Ready(x),
                y => panic!("current process was {:?}, not 'Running(_)'", y),
            };
            // current.current_thread = current.previous_context;
        }

        // Get the new process, and ensure that it is in a state where it's fit
        // to run.  Again, if the new process isn't fit to run, then the system
        // is in a very bad state.
        {
            let mut process = self.get_process_mut(pid)?;
            // Ensure the new context is available to be run
            let available_contexts = match process.state {
                ProcessState::Ready(x) if x & 1 << tid != 0 => x & !(1 << tid),
                other => panic!(
                    "process {} was in an invalid state {:?} -- thread {} not available to run",
                    pid, other, tid
                ),
            };
            process.state = ProcessState::Running(available_contexts);
            // process.current_thread = tid as u8;
            process.mapping.activate()?;
            process.activate()?;

            // Activate the current context
            let mut arch_process = crate::arch::process::Process::current();
            arch_process.set_thread(tid)?;
        }
        // self.pid = pid;
        Ok(())
    }

    // #[cfg(not(baremetal))]
    // pub fn make_callback_to(
    //     &mut self,
    //     _pid: PID,
    //     _pc: *const usize,
    //     _irq_no: usize,
    //     _arg: *mut usize,
    // ) -> Result<(), xous_kernel::Error> {
    //     Err(xous_kernel::Error::UnhandledSyscall)
    // }

    /// Create a stack frame in the specified process and jump to it.
    /// 1. Pause the current process and switch to the new one
    /// 2. Save the process state, if it hasn't already been saved
    /// 3. Run the new process, returning to an illegal instruction
    #[cfg(baremetal)]
    pub fn make_callback_to(
        &mut self,
        pid: PID,
        pc: *const usize,
        irq_no: usize,
        arg: *mut usize,
    ) -> Result<(), xous_kernel::Error> {
        // Get the current process (which was just interrupted) and mark it as
        // "ready to run".  If this function is called when the current process
        // isn't running, that means the system has gotten into an invalid
        // state.
        {
            let current_pid = self.current_pid();
            let mut current = self
                .get_process_mut(current_pid)
                .expect("couldn't get current PID");
            current.state = match current.state {
                ProcessState::Running(x) => {
                    ProcessState::Ready(x | (1 << arch::process::current_tid()))
                }
                y => panic!("current process was {:?}, not 'Running(_)'", y),
            };
            // println!("Making PID {} state {:?}", current_pid, current.state);
        }

        // Get the new process, and ensure that it is in a state where it's fit
        // to run.  Again, if the new process isn't fit to run, then the system
        // is in a very bad state.
        {
            let mut process = self.get_process_mut(pid)?;
            let available_threads = match process.state {
                ProcessState::Ready(x) | ProcessState::Running(x) => x,
                ProcessState::Sleeping => 0,
                ProcessState::Free => panic!("process was not allocated"),
                ProcessState::Setup(_) | ProcessState::Allocated => {
                    panic!("process hasn't been set up yet")
                }
            };
            process.state = ProcessState::Running(available_threads);
            process.previous_thread = process.current_thread;
            process.current_thread = arch::process::IRQ_TID;
            process.mapping.activate()?;
            process.activate()?;
        }

        // Switch to new process memory space, allowing us to save the context
        // if necessary.
        // self.pid = pid;

        // Invoke the syscall, but use the current stack pointer.  When this
        // function returns, it will jump to the RETURN_FROM_ISR address,
        // causing an instruction fault and exiting the interrupt.
        ArchProcess::with_current_mut(|arch_process| {
            let sp = arch_process.current_thread().stack_pointer();

            // Activate the current context
            arch_process.set_thread(arch::process::IRQ_TID).unwrap();

            // Construct the new frame
            arch::syscall::invoke(
                arch_process.current_thread_mut(),
                pid.get() == 1,
                pc as usize,
                sp,
                arch::process::RETURN_FROM_ISR,
                &[irq_no, arg as usize],
            );
        });
        Ok(())
    }

    /// Mark the specified context as ready to run. If the thread is Sleeping, mark
    /// it as Ready.
    pub fn ready_thread(&mut self, pid: PID, tid: TID) -> Result<(), xous_kernel::Error> {
        // assert!(tid == INITIAL_TID);
        let process = self.get_process_mut(pid)?;
        process.state = match process.state {
            ProcessState::Free => {
                panic!("PID {} was not running, so cannot wake thread {}", pid, tid)
            }
            ProcessState::Running(x) if x & (1 << tid) == 0 => {
                ProcessState::Running(x | (1 << tid))
            }
            ProcessState::Ready(x) if x & (1 << tid) == 0 => ProcessState::Ready(x | (1 << tid)),
            ProcessState::Sleeping => ProcessState::Ready(1 << tid),
            other => panic!(
                "PID {} was not in a state to wake thread {}: {:?}",
                pid, tid, other
            ),
        };
        // println!(
        //     "KERNEL({}): Readying context {} -> {:?}",
        //     pid, context, process.state
        // );
        Ok(())
    }

    /// Mark the current process as "Ready to run".
    ///
    /// # Panics
    ///
    /// If the current process is not running, or if it's "Running" but has no free contexts
    pub fn switch_to_thread(
        &mut self,
        pid: PID,
        tid: Option<TID>,
    ) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;
        // println!(
        //     "switch_to_thread({}:{:?}): Old state was {:?}",
        //     pid, tid, process.state
        // );

        // Determine which context number to switch to
        process.state = match process.state {
            ProcessState::Free => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::Sleeping => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::Allocated => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::Setup(setup) => {
                // Activate the process, which enables its memory mapping
                process.activate()?;

                // If a context is specified for a Setup task to switch to,
                // ensure it's the INITIAL_TID. Otherwise it's not valid.
                if let Some(tid) = tid {
                    if tid != INITIAL_TID {
                        panic!("switched to an incorrect thread");
                    }
                }

                let mut p = crate::arch::process::Process::current();
                p.setup_thread(INITIAL_TID, setup)?;
                p.set_thread(INITIAL_TID)?;
                ArchProcess::with_inner_mut(|process_inner| process_inner.pid = pid);
                // process.current_thread = INITIAL_TID as u8;

                // Mark the current proces state as "running, and no waiting contexts"
                ProcessState::Running(0)
            }
            ProcessState::Ready(0) => {
                panic!("ProcessState was `Ready(0)`, which is invalid!");
            }
            ProcessState::Ready(x) => {
                let new_thread = match tid {
                    None => {
                        let mut new_context = 0;

                        while x & (1 << new_context) == 0 {
                            new_context += 1;
                            if new_context > arch::process::MAX_THREAD {
                                new_context = 0;
                            }
                        }
                        new_context
                    }
                    Some(ctx) => {
                        // Ensure the specified context is ready to run
                        if x & (1 << ctx) == 0 {
                            return Err(xous_kernel::Error::InvalidThread);
                        }
                        ctx
                    }
                };

                process.activate()?;
                let mut p = crate::arch::process::Process::current();
                // FIXME: What happens if this fails? We're currently in the new process
                // but without a context to switch to.
                p.set_thread(new_thread)?;
                // process.current_thread = new_context as u8;

                // Remove the new context from the available context list
                ProcessState::Running(x & !(1 << new_thread))
            }
            ProcessState::Running(0) => {
                // TODO: If `context` is not `None`, what do we do here?

                // This process is already running, and there aren't any new available
                // contexts, so keep on going.
                ProcessState::Running(0)
            }
            ProcessState::Running(ready_threads) => {
                let mut p = crate::arch::process::Process::current();
                // let current_thread = p.current_thread();
                let new_thread = match tid {
                    None => {
                        let mut new_thread = 0;

                        while ready_threads & (1 << new_thread) == 0 {
                            new_thread += 1;
                            if new_thread > arch::process::MAX_THREAD {
                                new_thread = 0;
                            }
                        }
                        new_thread
                    }
                    Some(ctx) => {
                        // Ensure the specified context is ready to run, or is
                        // currently running.
                        if ready_threads & (1 << ctx) == 0
                        /*&& ctx != current_thread*/
                        {
                            return Err(xous_kernel::Error::InvalidThread);
                        }
                        ctx
                    }
                };

                // Remove the new thread ID from the list of thread IDs
                let new_mask = ready_threads & !(1 << new_thread);

                // Activate this process on this CPU
                process.activate()?;
                p.set_thread(new_thread)?;
                ProcessState::Running(new_mask)
            }
        };
        // println!(
        //     "switch_to_thread({}:{:?}): New state is {:?}",
        //     pid, tid, process.state
        // );
        Ok(())
    }

    /// Switches away from the specified process ID.
    /// If `can_resume` is `true`, then the current thread ID will be placed
    /// in the list of available thread IDs.
    /// If no thread IDs are available, the process will enter a `Sleeping` state.
    ///
    /// # Panics
    ///
    /// If the current process is not running.
    pub fn switch_from_thread(&mut self, pid: PID, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;
        // println!(
        //     "switch_from_thread({}:{}): Old state was {:?}",
        //     pid, tid, process.state
        // );
        process.state = match process.state {
            ProcessState::Running(x) if x & (1 << tid) != 0 => panic!(
                "PID {} thread {} was already queued for running when `switch_from_thread()` was called",
                pid, tid
            ),
            ProcessState::Running(0) => {
                if cfg!(baremetal) {
                    ProcessState::Sleeping
                } else {
                    ProcessState::Running(0)
                }
            }
            ProcessState::Running(x) => {
                if cfg!(baremetal) {
                    ProcessState::Ready(x)
                } else {
                    ProcessState::Running(x)
                }
            }
            other => {
                // ::debug_here::debug_here!();
                panic!(
                    "PID {} TID {} was not in a state to be switched from: {:?}",
                    pid, tid, other
                );
            },
        };
        // println!(
        //     "switch_from_thread({}:{}): New state is {:?}",
        //     pid, tid, process.state
        // );
        Ok(())
    }

    pub fn thread_is_running(&self, pid: PID, tid: TID) -> bool {
        let process = self.get_process(pid).unwrap();
        if let ProcessState::Running(thread_ids) = process.state {
            if thread_ids & (1 << tid) == 0 {
                return true;
            }
        }
        panic!("PID {} TID {} not running: {:?}", pid, tid, process.state);
        // match &process.state {
        //     &ProcessState::Sleeping => false,
        //     &ProcessState::Ready(_x) => false,
        //     &ProcessState::Free => false,
        //     &ProcessState::Running(x) if x & (1 << tid) != 0 => false,
        //     &ProcessState::Setup(_) => false,
        //     &ProcessState::Running(_) => true,
        // }
    }

    pub fn set_thread_result(
        &mut self,
        pid: PID,
        tid: TID,
        result: xous_kernel::Result,
    ) -> Result<(), xous_kernel::Error> {
        // Temporarily switch into the target process memory space
        // in order to pass the return value.
        let current_pid = self.current_pid();
        {
            let target_process = self.get_process(pid)?;
            target_process.activate()?;
            let mut arch_process = crate::arch::process::Process::current();
            arch_process.set_thread_result(tid, result);
        }

        // Return to the original memory space.
        let current_process = self
            .get_process(current_pid)
            .expect("couldn't switch back after setting context result");
        current_process.activate()?;
        Ok(())
    }

    /// Resume the given process, picking up exactly where it left off. If the
    /// process is in the Setup state, set it up and then resume.
    pub fn activate_process_thread(
        &mut self,
        previous_tid: TID,
        new_pid: PID,
        mut new_tid: TID,
        can_resume: bool,
    ) -> Result<TID, xous_kernel::Error> {
        let previous_pid = self.current_pid();
        // println!(
        //     "KERNEL({},{}): Activating process {} thread {}",
        //     previous_pid, previous_tid, new_pid, new_tid
        // );

        // Save state if the PID has changed.  This will activate the new memory
        // space.
        if new_pid != previous_pid {
            let new = self.get_process_mut(new_pid)?;
            // println!("New state: {:?}", new.state);

            // Ensure the new process can be run.
            match new.state {
                ProcessState::Free => {
                    println!("PID {} was free", new_pid);
                    return Err(xous_kernel::Error::ProcessNotFound);
                }
                ProcessState::Setup(_) | ProcessState::Allocated => new_tid = INITIAL_TID,
                ProcessState::Running(x) | ProcessState::Ready(x) => {
                    // If no new context is specified, take the previous
                    // context.  If that is not runnable, do a round-robin
                    // search for the next available context.
                    assert!(
                        x != 0,
                        "process was {:?} but had no free contexts",
                        new.state
                    );
                    if new_tid == 0 {
                        // print!(
                        //     "PID {}: Looking for a valid context in the mask {:08b}, curent context {} ({:08b})",
                        //     new_pid, x, new.current_context, new.current_context
                        // );
                        new_tid = 0; //new.current_thread as usize;
                        while x & (1 << new_tid) == 0 {
                            new_tid += 1;
                            if new_tid > arch::process::MAX_THREAD {
                                //     new_tid = 0;
                                // }
                                // // If we've looped around, return an error.
                                // if new_tid == new.current_thread as usize {
                                println!("Looked through all contexts and couldn't find one that was ready");
                                return Err(xous_kernel::Error::ProcessNotFound);
                            }
                        }
                    // println!(" -- picked thread {}", new_tid);
                    } else if x & (1 << new_tid) == 0 {
                        println!(
                            "thread is {:?}, which is not valid for new thread {}",
                            new.state, new_tid
                        );
                        return Err(xous_kernel::Error::ProcessNotFound);
                    }
                }
                ProcessState::Sleeping => {
                    println!("PID {} was sleeping", new_pid);
                    return Err(xous_kernel::Error::ProcessNotFound);
                }
            }

            // Perform the actual switch to the new memory space.  From this
            // point onward, we will need to activate the previous memory space
            // if we encounter an error.
            new.mapping.activate()?;

            // Set up the new process, if necessary.  Remove the new context from
            // the list of ready contexts.
            new.state = match new.state {
                ProcessState::Setup(thread_init) => {
                    // println!("Setting up new process...");
                    ArchProcess::setup_process(new_pid, thread_init)
                        .expect("couldn't set up new process");
                    ArchProcess::with_inner_mut(|process_inner| process_inner.pid = new_pid);

                    ProcessState::Running(0)
                }
                ProcessState::Allocated => {
                    ArchProcess::with_inner_mut(|process_inner| process_inner.pid = new_pid);
                    ProcessState::Running(0)
                }
                ProcessState::Free => panic!("process was suddenly Free"),
                ProcessState::Ready(x) | ProcessState::Running(x) => {
                    ProcessState::Running(x & !(1 << new_tid))
                }
                ProcessState::Sleeping => ProcessState::Running(0),
            };
            new.activate()?;

            // Mark the previous process as ready to run, since we just switched
            // away
            let previous = self
                .get_process_mut(previous_pid)
                .expect("couldn't get previous pid");
            previous.state = match previous.state {
                // If the previous process had exactly one thread that can be
                // run, then the Running thread list will be 0.  In that case,
                // we will either need to Sleep this process, or mark it as
                // being Ready to run.
                ProcessState::Running(x) if x == 0 => {
                    if can_resume {
                        ProcessState::Ready(1 << previous_tid)
                    } else {
                        ProcessState::Sleeping
                    }
                }
                // Otherwise, there are additional threads that can be run.
                // Convert the previous process into "Ready", and include the
                // current context number only if `can_resume` is `true`.
                ProcessState::Running(x) => {
                    // if can_resume {
                    //     ProcessState::Ready(x | (1 << previous_tid))
                    // } else {
                    ProcessState::Ready(x)
                    // }
                }
                other => panic!(
                    "previous process PID {} was in an invalid state (not Running): {:?}",
                    previous_pid, other
                ),
            };
        // if advance_thread {
        //     previous.current_thread += 1;
        //     if previous.current_thread as TID > arch::process::MAX_CONTEXT {
        //         previous.current_thread = 0;
        //     }
        // }
        // println!(
        //     "Set previous process PID {} state to {:?} (with can_resume = {})",
        //     previous_pid, previous.state, can_resume
        // );
        } else {
            // if self.current_thread(previous_pid) == new_tid {
            //     if !can_resume {
            //         panic!("tried to switch to our own context without resume");
            //     }
            //     return Ok(new_tid);
            // }
            let new = self.get_process_mut(new_pid)?; /*
                                                      new.state = match new.state {
                                                          ProcessState::Running(x) if (x & 1 << new_tid) == 0 => {
                                                              return Err(xous_kernel::Error::ProcessNotFound)
                                                          }
                                                          // ProcessState::Running(x) => {
                                                          //     if can_resume {
                                                          //         ProcessState::Running((x | (1 << previous_tid)) & !(1 << new_tid))
                                                          //     } else {
                                                          //         ProcessState::Running(x | (1 << previous_tid))
                                                          //     }
                                                          // }
                                                          other => */
            panic!(
                "PID {} invalid process state (not Running): {:?}",
                previous_pid, new.state
            ) /*,
              }*/
            ;
            // if advance_thread {
            //     new.current_thread += 1;
            //     if new.current_thread as TID > arch::process::MAX_CONTEXT {
            //         new.current_thread = 0;
            //     }
            // }
        }
        // self.pid = new_pid;

        let mut process = crate::arch::process::Process::current();

        // Restore the previous context, if one exists.
        process.set_thread(new_tid)?;
        // self.processes[new_pid.get() as usize - 1].current_thread = new_tid as u8;
        // let _ctx = process.current_context();

        Ok(new_tid)
    }

    /// Move memory from one process to another.
    ///
    /// During this process, memory is deallocated from the first process, then
    /// we switch contexts and look for a free slot in the second process. After
    /// that, we switch back to the first process and return.
    ///
    /// If no free slot can be found, memory is re-attached to the first
    /// process.  By following this break-then-make approach, we avoid getting
    /// into a situation where memory may appear in two different processes at
    /// once.
    ///
    /// The given memory range is guaranteed to be unavailable in the src process
    /// after this function returns.
    ///
    /// # Returns
    ///
    /// Returns the virtual address of the memory region in the target process.
    ///
    /// # Errors
    ///
    /// * **ShareViolation**: Tried to mutably share a region that was already
    ///   shared
    /// * **BadAddress**: The provided address was not valid
    /// * **BadAlignment**: The provided address or length was not page-aligned
    ///
    /// # Panics
    ///
    /// If the memory should have been able to go into the destination process
    /// but failed, then the system panics.
    #[cfg(baremetal)]
    pub fn send_memory(
        &mut self,
        src_virt: *mut u8,
        dest_pid: PID,
        dest_virt: *mut u8,
        len: usize,
    ) -> Result<*mut u8, xous_kernel::Error> {
        if len == 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if len & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if src_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if dest_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }

        let current_pid = self.current_pid();
        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        crate::mem::MemoryManager::with_mut(|mm| {
            // Locate an address to fit the new memory.
            dest_mapping.activate()?;
            let dest_virt = mm
                .find_virtual_address(dest_virt, len, xous_kernel::MemoryType::Messages)
                .or_else(|e| {
                    src_mapping.activate().expect("couldn't undo mapping");
                    Err(e)
                })?;
            src_mapping
                .activate()
                .expect("Couldn't switch back to source mapping");

            let mut error = None;

            // Lend each subsequent page.
            for offset in (0..(len / core::mem::size_of::<usize>()))
                .step_by(crate::mem::PAGE_SIZE / core::mem::size_of::<usize>())
            {
                mm.move_page(
                    &src_mapping,
                    src_virt.wrapping_add(offset),
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset),
                )
                .unwrap_or_else(|e| error = Some(e));
            }
            error.map_or_else(|| Ok(dest_virt), |e| panic!("unable to send: {:?}", e))
        })
    }

    #[cfg(not(baremetal))]
    pub fn send_memory(
        &mut self,
        src_virt: *mut u8,
        _dest_pid: PID,
        _dest_virt: *mut u8,
        _len: usize,
    ) -> Result<*mut u8, xous_kernel::Error> {
        Ok(src_virt)
    }

    /// Lend memory from one process to another.
    ///
    /// During this process, memory is marked as `Shared` in the source process.
    /// If the share is Mutable, then this memory is unmapped from the source
    /// process.  If the share is immutable, then memory is marked as
    /// not-writable in the source process.
    ///
    /// If no free slot can be found, memory is re-attached to the first
    /// process.  By following this break-then-make approach, we avoid getting
    /// into a situation where memory may appear in two different processes at
    /// once.
    ///
    /// If the share is mutable and the memory is already shared, then an error
    /// is returned.
    ///
    /// # Returns
    ///
    /// Returns the virtual address of the memory region in the target process.
    ///
    /// # Errors
    ///
    /// * **ShareViolation**: Tried to mutably share a region that was already
    ///   shared
    /// * **BadAddress**: The provided address was not valid
    /// * **BadAlignment**: The provided address or length was not page-aligned
    #[cfg(baremetal)]
    pub fn lend_memory(
        &mut self,
        src_virt: *mut u8,
        dest_pid: PID,
        dest_virt: *mut u8,
        len: usize,
        mutable: bool,
    ) -> Result<*mut u8, xous_kernel::Error> {
        if len == 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if len & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        if src_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }
        if dest_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAlignment);
        }

        let current_pid = self.current_pid();
        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        use crate::mem::MemoryManager;
        MemoryManager::with_mut(|mm| {
            // Locate an address to fit the new memory.
            dest_mapping.activate()?;
            let dest_virt = mm
                .find_virtual_address(dest_virt, len, xous_kernel::MemoryType::Messages)
                .or_else(|e| {
                    src_mapping.activate().unwrap();
                    Err(e)
                })?;
            src_mapping.activate().unwrap();

            let mut error = None;

            // Lend each subsequent page.
            for offset in (0..(len / core::mem::size_of::<usize>()))
                .step_by(crate::mem::PAGE_SIZE / core::mem::size_of::<usize>())
            {
                mm.lend_page(
                    &src_mapping,
                    src_virt.wrapping_add(offset),
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset),
                    mutable,
                )
                .unwrap_or_else(|e| {
                    error = Some(e);
                    0
                });
            }
            error.map_or_else(|| Ok(dest_virt), |e| panic!("unable to lend: {:?}", e))
        })
    }

    #[cfg(not(baremetal))]
    pub fn lend_memory(
        &mut self,
        src_virt: *mut u8,
        _dest_pid: PID,
        _dest_virt: *mut u8,
        _len: usize,
        _mutable: bool,
    ) -> Result<*mut u8, xous_kernel::Error> {
        Ok(src_virt)
    }

    /// Return memory from one process back to another
    ///
    /// During this process, memory is unmapped from the source process.
    ///
    /// # Returns
    ///
    /// Returns the virtual address of the memory region in the target process.
    ///
    /// # Errors
    ///
    /// * **ShareViolation**: Tried to mutably share a region that was already shared
    #[cfg(baremetal)]
    pub fn return_memory(
        &mut self,
        src_virt: *mut u8,
        _src_tid: TID,
        dest_pid: PID,
        _dest_tid: TID,
        dest_virt: *mut u8,
        len: usize,
        _buf: MemoryRange,
    ) -> Result<*mut u8, xous_kernel::Error> {
        if len == 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if len & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if src_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }
        if dest_virt as usize & 0xfff != 0 {
            return Err(xous_kernel::Error::BadAddress);
        }

        let current_pid = self.current_pid();
        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        use crate::mem::MemoryManager;
        MemoryManager::with_mut(|mm| {
            let mut error = None;

            // Lend each subsequent page.
            for offset in (0..(len / core::mem::size_of::<usize>()))
                .step_by(crate::mem::PAGE_SIZE / core::mem::size_of::<usize>())
            {
                mm.unlend_page(
                    &src_mapping,
                    src_virt.wrapping_add(offset),
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset),
                )
                .unwrap_or_else(|e| {
                    error = Some(e);
                    0
                });
            }
            error.map_or_else(|| Ok(dest_virt), |e| Err(e))
        })
    }

    #[cfg(not(baremetal))]
    pub fn return_memory(
        &mut self,
        src_virt: *mut u8,
        _src_tid: TID,
        dest_pid: PID,
        dest_tid: TID,
        _dest_virt: *mut u8,
        _len: usize,
        buf: MemoryRange,
    ) -> Result<*mut u8, xous_kernel::Error> {
        let buf = unsafe { core::slice::from_raw_parts(buf.as_ptr(), buf.len()) };
        let current_pid = self.current_pid();
        {
            let target_process = self.get_process(dest_pid)?;
            target_process.activate()?;
            let mut arch_process = crate::arch::process::Process::current();
            arch_process.return_memory(dest_tid, buf);
        }
        let target_process = self.get_process(current_pid)?;
        target_process.activate()?;

        Ok(src_virt)
    }

    /// Create a new thread in the current process.  Execution begins at
    /// `entrypoint`, with the stack pointer set to `stack_pointer`.  A single
    /// argument will be passed to the new function.
    ///
    /// The return address of this thread will be `EXIT_THREAD`, which the
    /// kernel can trap on to indicate a thread exited.
    ///
    /// # Errors
    ///
    /// * **ThreadNotAvailable**: The process has used all of its context
    ///   slots.
    pub fn create_thread(
        &mut self,
        pid: PID,
        thread_init: ThreadInit,
    ) -> Result<TID, xous_kernel::Error> {
        let mut process = self.get_process_mut(pid)?;
        process.activate()?;

        let mut arch_process = crate::arch::process::Process::current();
        let new_tid = arch_process
            .find_free_thread()
            .ok_or(xous_kernel::Error::ThreadNotAvailable)?;

        arch_process.setup_thread(new_tid, thread_init)?;

        // println!("KERNEL({}): Created new thread {}", pid, new_tid);

        // Queue the thread to run
        process.state = match process.state {
            ProcessState::Running(x) => ProcessState::Running(x | (1 << new_tid)),

            // This is the initial thread in this process -- schedule it to be run.
            ProcessState::Allocated => {
                ArchProcess::with_inner_mut(|process_inner| process_inner.pid = pid);
                ProcessState::Ready(1 << new_tid)
            }

            other => panic!(
                "error spawning thread: process was in an invalid state {:?}",
                other
            ),
        };

        Ok(new_tid)
    }

    /// Allocate a new server ID for this process and return the address. If the
    /// server table is full, or if there is not enough memory to map the server queue,
    /// return an error.
    ///
    /// # Errors
    ///
    /// * **OutOfMemory**: A new page could not be assigned to store the server
    ///   queue.
    /// * **ServerNotFound**: The server queue was full and a free slot could not
    ///   be found.
    pub fn create_server_with_address(
        &mut self,
        pid: PID,
        sid: SID,
    ) -> Result<(SID, CID), xous_kernel::Error> {
        // println!(
        //     "KERNEL({}): Looking through server list for free server",
        //     self.pid.get()
        // );

        // TODO: Come up with a way to randomize the server ID
        let ppid = self.get_process(pid)?.ppid.get();
        if ppid != 1 {
            panic!(
                "KERNEL({}): Non-PID1 processes cannot start servers yet",
                pid.get()
            );
        }

        for entry in self.servers.iter_mut() {
            if entry == &None {
                #[cfg(baremetal)]
                // Allocate a single page for the server queue
                let backing = crate::mem::MemoryManager::with_mut(|mm| {
                    MemoryRange::new(
                        mm.map_zeroed_page(pid, false)? as _,
                        crate::arch::mem::PAGE_SIZE,
                    )
                })?;

                #[cfg(not(baremetal))]
                let backing = MemoryRange::new(4096, 4096).unwrap();
                // println!(
                //     "KERNEL({}): Found a free slot for server {:?} @ {} -- allocating an entry",
                //     pid.get(),
                //     sid,
                //     _idx,
                // );

                // Initialize the server with the given memory page.
                Server::init(entry, pid, sid, backing).map_err(|x| x)?;

                let cid = self.connect_to_server(sid)?;
                return Ok((sid, cid));
            }
        }
        Err(xous_kernel::Error::ServerNotFound)
    }

    /// Generate a new server ID for this process and then create a new server.
    /// If the
    /// server table is full, or if there is not enough memory to map the server queue,
    /// return an error.
    ///
    /// # Errors
    ///
    /// * **OutOfMemory**: A new page could not be assigned to store the server
    ///   queue.
    /// * **ServerNotFound**: The server queue was full and a free slot could not
    ///   be found.
    pub fn create_server(
        &mut self,
        pid: PID,
    ) -> Result<(SID, CID), xous_kernel::Error> {
        let sid = SID::from_u32(arch::rand::get_u32(), arch::rand::get_u32(), arch::rand::get_u32(), arch::rand::get_u32());
        self.create_server_with_address(pid, sid)
    }

    /// Generate a random server ID and return it to the caller. Doesn't create
    /// any processes.
    pub fn create_server_id(&mut self) -> Result<SID, xous_kernel::Error> {
        let sid = SID::from_u32(arch::rand::get_u32(), arch::rand::get_u32(), arch::rand::get_u32(), arch::rand::get_u32());
        Ok(sid)
    }

    /// Connect to a server on behalf of another process.
    pub fn connect_process_to_server(&mut self, target_pid: PID, sid: SID)  -> Result<CID, xous_kernel::Error>{
        let original_pid = crate::arch::process::current_pid();

        let process = self.get_process_mut(target_pid)?;
        process.activate()?;

        let result = self.connect_to_server(sid);

        let process = self.get_process_mut(original_pid)?;
        process.activate().unwrap();

        result
    }
    /// Allocate a new server ID for this process and return the address. If the
    /// server table is full, return an error.
    pub fn connect_to_server(&mut self, sid: SID) -> Result<CID, xous_kernel::Error> {
        // Check to see if we've already connected to this server.
        // While doing this, find a free slot in case we haven't
        // yet connected.

        let pid = crate::arch::process::current_pid();
        // println!("KERNEL({}): Server table: {:?}", _pid.get(), self.servers);
        ArchProcess::with_inner_mut(|process_inner| {
            assert_eq!(pid, process_inner.pid);
            let mut slot_idx = None;
            // Look through the connection map for (1) a free slot, and (2) an
            // existing connection
            for (connection_idx, server_idx) in process_inner.connection_map.iter().enumerate() {
                // If we find an empty slot, use it
                if server_idx.is_none() {
                    if slot_idx.is_none() {
                        slot_idx = Some(connection_idx);
                    }
                    continue;
                }

                // If a connection to this server ID exists already, return it.
                let server_idx = (server_idx.unwrap().get() as usize) - 2;
                if let Some(allocated_server) = &self.servers[server_idx] {
                    if allocated_server.sid == sid {
                        // println!("KERNEL({}): Existing connection to SID {:?} found in this process @ {}, process connection map is: {:?}",
                        //     _pid.get(),
                        //     sid,
                        //     (connection_idx as CID) + 2,
                        //     process_inner.connection_map,
                        // );
                        return Ok((connection_idx as CID) + 2);
                    }
                }
            }
            let slot_idx = slot_idx.ok_or_else(|| Error::OutOfMemory)?;

            // Look through all servers for one whose SID matches.
            for (server_idx, server) in self.servers.iter().enumerate() {
                if let Some(allocated_server) = server {
                    if allocated_server.sid == sid {
                        process_inner.connection_map[slot_idx] =
                            Some(NonZeroU8::new((server_idx as u8) + 2).unwrap());
                        // println!(
                        //     "KERNEL({}): New connection to {:?}. After connection, cid is {} and process connection map is: {:?}",
                        //     pid.get(),
                        //     sid,
                        //     slot_idx + 2,
                        //     process_inner.connection_map
                        // );
                        return Ok((slot_idx as CID) + 2);
                    }
                }
            }
            Err(xous_kernel::Error::ServerNotFound) // May also be OutOfMemory if the table is full
        })
    }

    /// Retrieve the server ID index from the specified SID.
    /// This may only be called if the SID is a server owned by
    /// the current process.
    pub fn sidx_from_sid(&mut self, sid: SID, pid: PID) -> Option<usize> {
        // println!("KERNEL({}): Server table: {:?}", pid.get(), self.servers);
        for (idx, slot) in self.servers.iter().enumerate() {
            if let Some(server) = slot {
                if server.pid == pid && server.sid == sid {
                    return Some(idx);
                }
            }
        }
        None
    }

    /// Return a server based on the connection id and the current process
    pub fn server_from_sidx(&self, sidx: usize) -> Option<&Server> {
        if sidx > self.servers.len() {
            None
        } else {
            self.servers[sidx].as_ref()
        }
    }

    /// Return a server based on the connection id and the current process
    pub fn server_from_sidx_mut(&mut self, sidx: usize) -> Option<&mut Server> {
        if sidx > self.servers.len() {
            None
        } else {
            self.servers[sidx].as_mut()
        }
    }

    /// Retrieve a Server ID (Extended) value from the given Connection ID
    /// within the current process.
    pub fn sidx_from_cid(&self, cid: CID) -> Option<usize> {
        // println!("KERNEL({}): Attempting to get SIDX from CID {}", crate::arch::process::current_pid(), cid);
        if cid == 0 {
            // println!("KERNEL({}): CID is invalid -- returning", crate::arch::process::current_pid());
            return None;
        } else if cid == 1 {
            // println!("KERNEL({}): Server has terminated -- returning", crate::arch::process::current_pid());
            return None;
        }

        let cid = cid - 2;

        ArchProcess::with_inner(|process_inner| {
            assert_eq!(crate::arch::process::current_pid(), process_inner.pid);
            if (cid as usize) >= process_inner.connection_map.len() {
                // println!("KERNEL({}): CID {} > connection map len", crate::arch::process::current_pid(), cid);
                return None;
            }
            // if process_inner.connection_map[cid].is_none() {
            //     println!("KERNEL({}): CID {} doesn't exist in the connection map", crate::arch::process::current_pid(), cid + 2);
            //     println!("KERNEL({}): Process inner is: {:?}", crate::arch::process::current_pid(), process_inner);
            // }
            let mut server_idx = process_inner.connection_map[cid as usize]?.get() as usize;
            if server_idx == 1 {
                // println!("KERNEL({}): CID {} is no longer valid", crate::arch::process::current_pid(), cid + 2);
                return None;
            }
            server_idx -= 2;
            if server_idx >= self.servers.len() {
                // println!("KERNEL({}): CID {} and server_idx >= {}", crate::arch::process::current_pid(), cid + 2, server_idx);
                None
            } else {
                // println!("KERNEL({}): SIDX for CID {} found at index {}", crate::arch::process::current_pid(), cid + 2, server_idx);
                Some(server_idx)
            }
        })
    }

    /// Switch to the server's memory space and add the message to its server
    /// queue
    pub fn queue_server_message(
        &mut self,
        sidx: usize,
        pid: PID,
        context: TID,
        message: Message,
        original_address: Option<MemoryAddress>,
    ) -> Result<usize, xous_kernel::Error> {
        let current_pid = self.current_pid();
        let result = {
            let server_pid = self
                .server_from_sidx(sidx)
                .ok_or(xous_kernel::Error::ServerNotFound)?
                .pid;
            {
                let server_process = self.get_process(server_pid)?;
                server_process.mapping.activate().unwrap();
            }
            let server = self
                .server_from_sidx_mut(sidx)
                .expect("couldn't re-discover server index");
            server.queue_message(pid, context, message, original_address)
        };
        let current_process = self
            .get_process(current_pid)
            .expect("couldn't restore previous process");
        current_process.mapping.activate()?;
        result
    }

    /// Switch to the server's address space and add a "remember this address"
    /// entry to its server queue, then switch back to the original address space.
    pub fn remember_server_message(
        &mut self,
        sidx: usize,
        current_pid: PID,
        current_thread: TID,
        message: &Message,
        client_address: Option<MemoryAddress>,
    ) -> Result<usize, xous_kernel::Error> {
        let server_pid = self
            .server_from_sidx(sidx)
            .ok_or(xous_kernel::Error::ServerNotFound)?
            .pid;
        {
            let server_process = self.get_process(server_pid)?;
            server_process.mapping.activate()?;
        }
        let server = self
            .server_from_sidx_mut(sidx)
            .expect("couldn't re-discover server index");
        let result = server.queue_response(current_pid, current_thread, message, client_address);
        let current_process = self
            .get_process(current_pid)
            .expect("couldn't find old process");
        current_process
            .mapping
            .activate()
            .expect("couldn't switch back to previous address space");
        result
    }

    // /// Get a server index based on a SID
    // pub fn server_sidx(&mut self, sid: SID) -> Option<usize> {
    //     for (idx, server) in self.servers.iter_mut().enumerate() {
    //         if let Some(active_server) = server {
    //             if active_server.sid == sid {
    //                 return Some(idx);
    //             }
    //         }
    //     }
    //     None
    // }

    /// Terminate the given process. Returns the process' parent PID.
    pub fn terminate_process(&mut self, target_pid: PID) -> Result<PID, xous_kernel::Error> {
        // To terminate a process, we must perform the following:
        //
        // 1. If we have any client connections, remove them.
        // 2. If there are any clients connected to our server, insert a tombstone so writes fail
        // 3. If there are any incoming server requests queued, dequeue them and return an error
        // 4. Mark all "Borrowed" memory as "Free-when-returned". That way, if we've shared
        //    memory to a Server, it will be reclaimed by the system when it comes back

        // 1. Find all servers associated with this PID and remove them.
        for (idx, server) in self.servers.iter_mut().enumerate() {
            if let Some(server) = server {
                if server.pid == target_pid {
                    // This is our server, so look through the connection map of each
                    // process to determine if this connection needs to be replaced
                    // with a tombstone.
                    for process in self.processes.iter() {
                        if process.free() {
                            continue;
                        }
                        process.activate()?;
                        ArchProcess::with_inner_mut(|process_inner| {
                            // Look through the connection map for a connection
                            // that matches this index. Note that connection map entries
                            // are offset by two, because 0 == free and 1 == "tombstone".
                            for mapping in process_inner.connection_map.iter_mut() {
                                if let Some(mapping) = mapping {
                                    if mapping.get() == (idx as u8) + 2 {
                                        *mapping = NonZeroU8::new(1).unwrap();
                                    }
                                }
                            }
                        })
                    }
                }

                // Look through this server's memory space to determine if this process
                // is mentioned there as having some memory lent out.
                server.discard_messages_for_pid(target_pid);
            }
        }
        let process = self.get_process_mut(target_pid)?;
        process.activate()?;
        let parent_pid = process.ppid;
        process.terminate()?;
        // println!("KERNEL({}): Terminated", target_pid);

        let process = self.get_process(parent_pid)?;
        process.activate().unwrap();

        Ok(parent_pid)
    }

    /// Calls the provided function with the current inner process state.
    pub fn shutdown(&mut self) -> Result<(), xous_kernel::Error> {
        // Destroy all servers. This will cause all queued messages to be lost.
        for server in &mut self.servers {
            if server.is_some() {
                Server::destroy(server).unwrap();
            }
        }

        // Destroy all processes. This will cause them to immediately terminate.
        for process in &mut self.processes {
            if !process.free() {
                process.activate().unwrap();
                process.terminate().unwrap();
            }
        }
        Ok(())
    }
}
