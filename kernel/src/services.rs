// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::arch;
use crate::arch::mem::MemoryMapping;
pub use crate::arch::process::Process as ArchProcess;
pub use crate::arch::process::Thread;
use crate::platform;
use xous_kernel::arch::ProcessStartup;
use xous_kernel::MemoryRange;

use core::num::NonZeroU8;

use crate::filled_array;
use crate::server::Server;
// use core::mem;
use xous_kernel::{
    pid_from_usize, Error, MemoryAddress, Message, ProcessInit, ThreadInit, CID, PID, SID, TID,
};

const MAX_SERVER_COUNT: usize = 128;

pub use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT};

#[allow(dead_code)]
const MINIELF_FLG_W: u8 = 1;
#[allow(dead_code)]
const MINIELF_FLG_NC: u8 = 2;
#[allow(dead_code)]
const MINIELF_FLG_X: u8 = 4;
#[cfg(baremetal)]
const MINIELF_FLG_EHF: u8 = 8;
#[cfg(baremetal)]
const MINIELF_FLG_EHH: u8 = 0x10;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ExceptionHandler {
    /// Address (in program space) where the exception handler is
    pub pc: usize,

    /// Stack pointer when the exception is run
    pub sp: usize,
}

// fn log_process_update(f: &str, l: u32, process: &Process, old_state: ProcessState) {
//     if process.pid.get() == 3 {
//         println!("[{}:{}] Updated PID {:?} state: {:?} -> {:?}", f, l, process.pid, old_state, process.state);
//     }
// }

/// A big unifying struct containing all of the system state.
/// This is inherited from the stage 1 bootloader.
pub struct SystemServices {
    /// A table of all processes in the system
    pub processes: [Process; MAX_PROCESS_COUNT],

    /// A table of all servers in the system
    pub servers: [Option<Server>; MAX_SERVER_COUNT],
}

#[derive(Copy, Clone, PartialEq)]
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
    /// interrupt.  There are no contexts that can be run. This is
    /// functionally equivalent to the invalid `Ready(0)` state.
    Sleeping,

    /// The process is currently being debugged. When it is resumed,
    /// this will turn into `Ready(usize)`
    #[cfg(feature = "gdb-stub")]
    Debug(usize),

    /// The process is currently being debugged, but an interrupt happened
    /// anyway. When the interrupt finishes, this will turn into `Debug(usize)`.
    /// This generally should not happen, but is here in case NMIs are ever a thing.
    #[cfg(feature = "gdb-stub")]
    DebugIrq(usize),

    /// This process is processing an exception. When it is resumed, it will
    /// turn into `Ready(usize)`.
    Exception(usize),

    /// This process is processing an exception, but is waiting for a response from
    /// another Server. When it is resumed, it will turn into `Exception(usize)`.
    BlockedException(usize),
}

impl core::fmt::Debug for ProcessState {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        use ProcessState::*;
        match *self {
            Free => write!(fmt, "Free"),
            Allocated => write!(fmt, "Allocated"),
            Setup(ti) => write!(fmt, "Setup({:?})", ti),
            Ready(rt) => write!(fmt, "Ready({:b})", rt),
            Running(rt) => write!(fmt, "Running({:b})", rt),
            #[cfg(feature = "gdb-stub")]
            Debug(rt) => write!(fmt, "Debug({:b})", rt),
            #[cfg(feature = "gdb-stub")]
            DebugIrq(rt) => write!(fmt, "DebugIrq({:b})", rt),
            Exception(rt) => write!(fmt, "Exception({:b})", rt),
            BlockedException(rt) => write!(fmt, "BlockedException({:b})", rt),
            Sleeping => write!(fmt, "Sleeping"),
        }
    }
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
    pub current_thread: TID,

    /// The context number that was active before this process was switched
    /// away.
    previous_thread: TID,

    /// When an exception is hit, the kernel will switch to this Thread.
    exception_handler: Option<ExceptionHandler>,
}

impl Default for Process {
    fn default() -> Self {
        Process {
            ppid: unsafe { PID::new_unchecked(1) },
            state: ProcessState::Allocated,
            pid: unsafe { PID::new_unchecked(2) },
            current_thread: 0,
            previous_thread: 0,
            exception_handler: None,
            mapping: Default::default(),
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
        matches!(
            self.state,
            ProcessState::Setup(_) | ProcessState::Ready(_) | ProcessState::Exception(_)
        )
    }

    /// This process slot is unallocated and may be turn into a process
    pub fn free(&self) -> bool {
        matches!(self.state, ProcessState::Free)
    }

    pub fn activate(&self) -> Result<(), xous_kernel::Error> {
        crate::arch::process::set_current_pid(self.pid);
        self.mapping.activate()?;
        let mut current_process = ArchProcess::current();
        current_process.activate()
    }

    pub fn terminate(&mut self) -> Result<(), xous_kernel::Error> {
        if self.free() {
            return Err(xous_kernel::Error::ProcessNotFound);
        }

        println!("[!] Terminating process with PID {}", self.pid);

        // Free all associated memory pages
        unsafe {
            crate::mem::MemoryManager::with_mut(|mm| mm.release_all_memory_for_process(self.pid))
        };

        // Free all claimed IRQs
        crate::irq::release_interrupts_for_pid(self.pid);

        // Remove this PID from the process table
        ArchProcess::destroy(self.pid)?;
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
        current_thread: 0_usize,
        previous_thread: INITIAL_TID as TID,
        exception_handler: None,
    }; MAX_PROCESS_COUNT],
    // Note we can't use MAX_SERVER_COUNT here because of how Rust's
    // macro tokenization works
    servers: filled_array![None; 128],
}));

#[cfg(baremetal)]
#[no_mangle]
static mut SYSTEM_SERVICES: SystemServices = SystemServices {
    processes: [Process {
        state: ProcessState::Free,
        ppid: unsafe { PID::new_unchecked(1) },
        pid: unsafe { PID::new_unchecked(1) },
        mapping: arch::mem::DEFAULT_MEMORY_MAPPING,
        current_thread: INITIAL_TID,
        previous_thread: INITIAL_TID as TID,
        exception_handler: None,
    }; MAX_PROCESS_COUNT],
    // Note we can't use MAX_SERVER_COUNT here because of how Rust's
    // macro tokenization works
    servers: filled_array![None; 128],
};

impl core::fmt::Debug for Process {
    fn fmt(&self, fmt: &mut core::fmt::Formatter) -> core::result::Result<(), core::fmt::Error> {
        write!(
            fmt,
            "Process {} state: {:?}  TID: {}  Memory mapping: {:?}",
            self.pid.get(),
            self.state,
            self.current_thread,
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
                if arg.name == u32::from_le_bytes(*b"IniE")
                    || arg.name == u32::from_le_bytes(*b"IniF")
                {
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

        // we can *only* iterate through kernel args. We can't access it as a slice.
        let mut arg_iter = args.iter().peekable();
        loop {
            if let Some(arg) = arg_iter.peek() {
                if arg.name == u32::from_le_bytes(*b"IniE") || arg.name == u32::from_le_bytes(*b"IniF") {
                    break;
                }
            } else {
                panic!("Could not re-discover start of user services");
            }
            arg_iter.next();
        }
        // at this point, arg.next() will have the offset of the first process argument.

        // Copy over the initial process list.  The pid is encoded in the SATP
        // value from the bootloader.  For each process, translate it from a raw
        // KernelArguments value to a SystemServices Process value.
        for init in init_offsets.iter() {
            let pid = init.pid().get();
            let proc_idx = pid - 1;
            let process = &mut self.processes[proc_idx as usize];
            // println!(
            //     "Process[{}]: {:?}",
            //     pid - 1,
            //     init,
            // );
            unsafe {
                process.mapping.from_init_process(*init);
                process.ppid = PID::new_unchecked(1);
                process.pid = PID::new(pid as _).unwrap();
            };
            // let old_state = process.state;
            if pid == 1 {
                process.state = ProcessState::Running(0);
            } else {
                // This code makes the following assumption:
                //   - Arguments are in the same order as processes
                //   - All processes take the form of IniE/IniF such that the flags are in the last word
                //   - Process #1 is the kernel
                // Any changes to this will break this code!
                let mut eh_frame = 0;
                let mut eh_frame_size = 0;
                let mut eh_frame_header = 0;
                let mut eh_frame_header_size = 0;
                if let Some(arg) = arg_iter.next() {
                    for section in arg.data.chunks_exact(2) {
                        let flags = (section[1] >> 24) as u8;
                        if flags & MINIELF_FLG_EHF != 0 {
                            eh_frame = section[0];
                            eh_frame_size = section[1] & 0xFF_FFFF;
                        } else if flags & MINIELF_FLG_EHH != 0 {
                            eh_frame_header = section[0];
                            eh_frame_header_size = section[1] & 0xFF_FFFF;
                        }
                    }
                }

                // end of assumption area
                process.state = ProcessState::Setup(ThreadInit::new(
                    unsafe { core::mem::transmute::<usize, _>(init.entrypoint) },
                    unsafe {
                        MemoryRange::new(
                            init.sp - crate::arch::process::DEFAULT_STACK_SIZE,
                            crate::arch::process::DEFAULT_STACK_SIZE,
                        )
                        .unwrap()
                    },
                    eh_frame as _,
                    eh_frame_size as _,
                    eh_frame_header as _,
                    eh_frame_header_size as _,
                ));
            }
            // log_process_update(file!(), line!(), process, old_state);
        }

        // Set up our handle with a bogus sp and pc.  These will get updated
        // once a context switch _away_ from the kernel occurs, however we need
        // to make sure other fields such as "thread number" are all valid.
        ArchProcess::setup_process(PID::new(1).unwrap(), ThreadInit::default())
            .expect("couldn't setup process");
    }

    /// Add a new entry to the process table. This results in a new address space
    /// and a new PID, though the process is in the state `Setup()`.
    pub fn create_process(
        &mut self,
        init_process: ProcessInit,
    ) -> Result<ProcessStartup, xous_kernel::Error> {
        let mut entry_idx = None;
        let mut new_pid = None;
        let _ppid = crate::arch::process::current_pid();

        for (idx, entry) in self.processes.iter_mut().enumerate() {
            if entry.state != ProcessState::Free {
                continue;
            }
            entry_idx = Some(idx);
            new_pid = Some(pid_from_usize(idx + 1)?);
            entry.pid = new_pid.unwrap();
            entry.ppid = PID::new(1).unwrap();
            entry.state = ProcessState::Allocated;
            unsafe {
                entry
                    .mapping
                    .allocate(new_pid.unwrap())
                    .or(Err(xous_kernel::Error::InternalError))?
            };
            break;
        }
        if entry_idx.is_none() {
            return Err(xous_kernel::Error::ProcessNotFound);
        }
        let new_pid = new_pid.unwrap();
        let startup = arch::process::Process::create(new_pid, init_process, self).unwrap();

        #[cfg(baremetal)]
        {
            let mut entry = &mut self.processes[entry_idx.unwrap()];
            // The `Process::create()` call above set up the process so that it will
            // be ready to run right away, meaning we will not need to first set
            // the state to `ProcessState::Allocated` and we can go straight to running
            // this process.
            entry.state = ProcessState::Ready(1 << INITIAL_TID);
        }
        // entry.ppid = _ppid;
        klog!(
            "created new process for PID {} with PPID {}",
            new_pid,
            _ppid
        );
        return Ok(startup);
    }

    pub fn get_process(&self, pid: PID) -> Result<&Process, xous_kernel::Error> {
        // PID0 doesn't exist -- process IDs are offset by 1.
        let pid_idx = pid.get() as usize - 1;
        if cfg!(baremetal) && self.processes[pid_idx].mapping.get_pid() != pid {
            klog!(
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
            klog!("Finishing callback in PID {}", current_pid);
            // let old_state = current.state;
            current.state = match current.state {
                ProcessState::Running(0) => ProcessState::Sleeping,
                ProcessState::Running(x) => ProcessState::Ready(x),
                #[cfg(feature = "gdb-stub")]
                ProcessState::DebugIrq(x) => ProcessState::Debug(x),
                y => panic!("current process was {:?}, not 'Running(_)'", y),
            };
            // log_process_update(file!(), line!(), current, old_state);
            // current.current_thread = current.previous_context;
        }

        // Get the new process, and ensure that it is in a state where it's fit
        // to run. Again, if the new process isn't fit to run, then the system
        // is in a very bad state.
        {
            let mut process = self.get_process_mut(pid)?;
            #[cfg(feature = "gdb-stub")]
            let ppid = process.ppid;
            // Ensure the new context is available to be run
            let available_threads = match process.state {
                ProcessState::Ready(x) if x & 1 << tid != 0 => x & !(1 << tid),
                // If we're currently debugging the process, return to its parent.
                // This can happen when the process handles a debug interrupt.
                #[cfg(feature = "gdb-stub")]
                ProcessState::Debug(_) => {
                    crate::syscall::reset_switchto_caller();
                    return self.switch_to_thread(ppid, None);
                }
                other => panic!(
                    "process {} was in an invalid state {:?} -- thread {} not available to run",
                    pid, other, tid
                ),
            };
            process.state = ProcessState::Running(available_threads);
            // klog!(
            //     "in resuming callback, process state went from {:?} to {:?}",
            //     old_state,
            //     process.state
            // );
            // log_process_update(file!(), line!(), process, old_state);
            // process.current_thread = tid as u8;
            process.mapping.activate()?;
            process.activate()?;

            // Activate the current context
            ArchProcess::current().set_tid(tid)?;
            process.current_thread = tid;
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
            // The current thread should never be 0, but for some reason it ends up
            // as 0 after resuming from suspend. It's unclear how this happens.
            if current.current_thread == 0 {
                current.current_thread = arch::process::current_tid();
            }
            // let old_state = current.state;
            current.state = match current.state {
                ProcessState::Running(x) => ProcessState::Ready(x | (1 << current.current_thread)),
                y => panic!("current process was {:?}, not 'Running(_)'", y),
            };
            // log_process_update(file!(), line!(), current, old_state);
            // println!("Making PID {} state {:?}", current_pid, current.state);
        }

        // Get the new process, and ensure that it is in a state where it's fit
        // to run.  Again, if the new process isn't fit to run, then the system
        // is in a very bad state.
        {
            let mut process = self.get_process_mut(pid)?;
            let available_threads = match process.state {
                ProcessState::Ready(x) | ProcessState::Running(x) | ProcessState::Exception(x) => x,
                #[cfg(feature = "gdb-stub")]
                ProcessState::Debug(x) | ProcessState::DebugIrq(x) => x,
                ProcessState::Sleeping | ProcessState::BlockedException(_) => 0,
                ProcessState::Free => panic!("process was not allocated"),
                ProcessState::Setup(_) | ProcessState::Allocated => {
                    panic!("process hasn't been set up yet")
                }
            };
            #[cfg(feature = "gdb-stub")]
            if let ProcessState::Debug(_) = process.state {
                println!("Making a callback for IRQ {} to process {:?} which is currently in a debug state!", irq_no, pid);
                process.state = ProcessState::DebugIrq(available_threads);
            } else {
                process.state = ProcessState::Running(available_threads);
            }

            // let old_state = process.state;
            #[cfg(not(feature = "gdb-stub"))]
            {
                process.state = ProcessState::Running(available_threads);
            }

            // log_process_update(file!(), line!(), process, old_state);
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
            let sp = if pid.get() == 1 {
                arch::mem::EXCEPTION_STACK_TOP
            } else {
                arch_process.current_thread().stack_pointer()
            };

            // Activate the current context
            arch_process.set_tid(arch::process::IRQ_TID).unwrap();

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

    pub fn runnable(&self, pid: PID, tid: Option<TID>) -> Result<bool, xous_kernel::Error> {
        let process = self.get_process(pid)?;
        if let Some(tid) = tid {
            Ok(match process.state {
                ProcessState::Running(x) => tid == process.current_thread || x & (1 << tid) != 0,
                ProcessState::Ready(x) if x & (1 << tid) != 0 => true,
                ProcessState::Sleeping => true,
                _ => false,
            })
        } else {
            Ok(matches!(
                process.state,
                ProcessState::Running(_) | ProcessState::Ready(_) | ProcessState::Sleeping
            ))
        }
    }

    /// Mark the specified context as ready to run. If the thread is Sleeping, mark
    /// it as Ready.
    pub fn ready_thread(&mut self, pid: PID, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;
        // let old_state = process.state;
        process.state = match process.state {
            ProcessState::Free => {
                panic!("PID {} was not running, so cannot wake thread {}", pid, tid)
            }
            ProcessState::Running(x) if x & (1 << tid) == 0 => {
                ProcessState::Running(x | (1 << tid))
            }
            ProcessState::Ready(x) if x & (1 << tid) == 0 => ProcessState::Ready(x | (1 << tid)),
            ProcessState::Sleeping => ProcessState::Ready(1 << tid),
            #[cfg(feature = "gdb-stub")]
            ProcessState::Debug(x) if x & (1 << tid) == 0 => ProcessState::Debug(x | (1 << tid)),
            #[cfg(feature = "gdb-stub")]
            ProcessState::DebugIrq(x) if x & (1 << tid) == 0 => {
                ProcessState::DebugIrq(x | (1 << tid))
            }
            ProcessState::Exception(ready_threads)
            | ProcessState::BlockedException(ready_threads) => {
                ProcessState::Exception(ready_threads)
            }
            other => panic!(
                "PID {} was not in a state to wake thread {}: {:?}",
                pid, tid, other
            ),
        };
        // log_process_update(file!(), line!(), process, old_state);
        klog!("Readying ({}:{}) -> {:?}", pid, tid, process.state);
        Ok(())
    }

    #[cfg(target_pointer_width = "32")]
    pub fn find_next_thread(thread_mask: usize, current_thread: usize) -> usize {
        // From https://graphics.stanford.edu/~seander/bithacks.html#ZerosOnRightMultLookup
        // This platform has a multiplier, so this is fast
        fn trailing_zeros(v: usize) -> usize {
            const MULTIPLY_DEBRUIJN_BIT_POSITION: [usize; 32] = [
                0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19,
                16, 7, 26, 12, 18, 6, 11, 5, 10, 9,
            ];

            MULTIPLY_DEBRUIJN_BIT_POSITION[((!v.wrapping_sub(1) & v) * 0x077CB531) >> 27]
        }
        // If there's only one thread runnable, run that one
        if thread_mask == 0 {
            panic!("no threads were available to run");
        }

        // if thread_mask.is_power_of_two() {
        if thread_mask & (thread_mask - 1) == 0 {
            trailing_zeros(thread_mask)
        } else {
            let upper_bits = thread_mask & !((2usize << current_thread) - 1);
            if upper_bits != 0 {
                trailing_zeros(upper_bits)
            } else {
                trailing_zeros(thread_mask)
            }
        }
    }

    #[cfg(not(target_pointer_width = "32"))]
    pub fn find_next_thread(thread_mask: usize, current_thread: usize) -> usize {
        if thread_mask == 0 {
            panic!("no threads were available to run");
        }
        if thread_mask.is_power_of_two() {
            thread_mask.trailing_zeros() as usize
        } else {
            let upper_bits = thread_mask & !((2usize << current_thread) - 1);
            if upper_bits != 0 {
                upper_bits.trailing_zeros() as usize
            } else {
                thread_mask.trailing_zeros() as usize
            }
        }
    }

    /// Set the "current thread" of a given process. It is designed
    /// to set where the next thread will run in order to avoid starving threads
    /// when messages are passed around.
    /// For example, if there are three threads (1, 2, 3), then there is a case where
    /// thread 1 sends a message to thread 3, which yields. In this case, thread 1 will be
    /// picked to run next, completely skipping thread 2.
    /// Use `set_last_thread()` when a process' quantum is up in order to tell the scheduler
    /// which thread to use as a reference for picking the next runnable thread.
    pub fn set_last_thread(&mut self, pid: PID, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;

        match process.state {
            ProcessState::Ready(runnable) if runnable & (1 << tid) != 0 => {
                process.current_thread = tid;
                Ok(())
            }
            ProcessState::Ready(_) | ProcessState::Sleeping => {
                Err(xous_kernel::Error::ThreadNotAvailable)
            }
            ProcessState::Running(_) => panic!("thread was still running"),
            _ => Err(xous_kernel::Error::ProcessNotFound),
        }
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
        // klog!(
        //     "switch_to_thread({}:{:?}): Old state was {:?}",
        //     pid, tid, process.state
        // );

        // let old_state = process.state;
        // Determine which thread to switch to
        process.state = match process.state {
            ProcessState::Free => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::Sleeping => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::Allocated => return Err(xous_kernel::Error::ProcessNotFound),
            ProcessState::BlockedException(_) => {
                panic!("tried to switch to an exception handler that was blocked")
            }
            #[cfg(feature = "gdb-stub")]
            ProcessState::Debug(_) | ProcessState::DebugIrq(_) => {
                return Err(xous_kernel::Error::DebugInProgress)
            }
            ProcessState::Exception(x) => {
                process.activate()?;
                let mut p = ArchProcess::current();
                let tid = crate::arch::process::EXCEPTION_TID;
                p.set_tid(tid)?;
                // process.current_thread = tid as _;
                ProcessState::Exception(x)
            }
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

                let mut p = ArchProcess::current();
                p.setup_thread(INITIAL_TID, setup)?;
                p.set_tid(INITIAL_TID)?;
                ArchProcess::with_inner_mut(|process_inner| process_inner.pid = pid);
                process.current_thread = INITIAL_TID as _;

                // Mark the current proces state as "running, and no waiting contexts"
                ProcessState::Running(0)
            }
            ProcessState::Ready(0) => {
                panic!("ProcessState was `Ready(0)`, which is invalid!");
            }
            ProcessState::Ready(ready_threads) => {
                let new_thread = tid.unwrap_or_else(|| {
                    Self::find_next_thread(ready_threads, process.current_thread)
                });

                if ready_threads & (1 << new_thread) == 0 {
                    panic!("invalid thread ID");
                }

                process.activate()?;

                ArchProcess::current().set_tid(new_thread)?;
                process.current_thread = new_thread as _;
                ProcessState::Running(ready_threads & !(1 << new_thread))
            }
            ProcessState::Running(ready_threads) => {
                // Ensure we can switch back to this thread, if necessary
                let ready_threads = ready_threads | (1 << process.current_thread);

                let new_thread = tid.unwrap_or_else(|| {
                    Self::find_next_thread(ready_threads, process.current_thread)
                });

                // Ensure the specified context is ready to run, or is
                // currently running.
                if ready_threads & (1 << new_thread) == 0 {
                    return Err(xous_kernel::Error::InvalidThread);
                }

                // Activate this process on this CPU
                #[cfg(not(target_os = "xous"))]
                process.activate()?;
                ArchProcess::current().set_tid(new_thread)?;
                process.current_thread = new_thread as _;
                ProcessState::Running(ready_threads & !(1 << new_thread))
            }
        };
        // log_process_update(file!(), line!(), process, old_state);

        // println!(
        //     "switch_to_thread({}:{:?}): New state is {:?} Thread is ",
        //     pid, tid, process.state
        // );
        // ArchProcess::with_current(|current| current.print_thread());

        Ok(())
    }

    /// Switches away from the specified process ID and ensures it won't
    /// get scheduled again.
    /// If no thread IDs are available, the process will enter a `Sleeping` state.
    ///
    /// # Panics
    ///
    /// If the current process is not running.
    pub fn unschedule_thread(&mut self, pid: PID, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;
        // klog!(
        //     "unschedule_thread({}:{}): Old state was {:?}",
        //     pid, tid, process.state
        // );
        // ArchProcess::with_current(|current| current.print_thread());

        // let old_state = process.state;
        process.state = match process.state {
            ProcessState::Running(x) if x & (1 << tid) != 0 => panic!(
                "PID {} thread {} was already queued for running when `unschedule_thread()` was called",
                pid, tid
            ),
            ProcessState::Running(0) => {
                if cfg!(baremetal) {
                    ProcessState::Sleeping
                } else {
                    ProcessState::Running(0)
                }
            }
            ProcessState::Exception(x) => ProcessState::BlockedException(x),
            ProcessState::Running(x) => {
                if cfg!(baremetal) {
                    ProcessState::Ready(x)
                } else {
                    ProcessState::Running(x)
                }
            }
            other => {
                panic!(
                    "PID {} TID {} was not in a state to be switched from: {:?}",
                    pid, tid, other
                );
            },
        };
        // log_process_update(file!(), line!(), process, old_state);
        // klog!(
        //     "unschedule_thread({}:{}): New state is {:?}",
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
        if current_pid == pid {
            ArchProcess::current().set_thread_result(tid, result);
            return Ok(());
        }

        {
            let target_process = self.get_process(pid)?;
            target_process.activate()?;
            let mut arch_process = ArchProcess::current();
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

        #[cfg(feature = "debug-print")]
        if new_tid != 0 {
            klog!("Activating process {} thread {}", new_pid, new_tid);
        } else {
            klog!("Activating process {} thread ANY", new_pid);
        }

        // Save state if the PID has changed.  This will activate the new memory
        // space.
        let new = self.get_process_mut(new_pid)?;
        if new_pid != previous_pid {
            klog!("New process original state: {:?}", new.state);

            // Ensure the new process can be run.
            match new.state {
                ProcessState::Free => {
                    klog!("PID {} was free", new_pid);
                    return Err(xous_kernel::Error::ProcessNotFound);
                }
                ProcessState::Setup(_) | ProcessState::Allocated => new_tid = INITIAL_TID,
                ProcessState::Exception(_) => {
                    new_tid = crate::arch::process::EXCEPTION_TID;
                    // new.current_thread = new_tid;
                }
                ProcessState::Ready(x) => {
                    // If no new context is specified, take the previous
                    // context.  If that is not runnable, do a round-robin
                    // search for the next available context.
                    assert!(
                        x != 0,
                        "process was {:?} but had no runnable threads",
                        new.state
                    );
                    if new_tid == 0 {
                        new_tid = Self::find_next_thread(x, new.current_thread);
                    }
                    if x & (1 << new_tid) == 0 {
                        println!(
                            "process state is {:?}, but new thread {} is not runnable",
                            new.state, new_tid
                        );
                        return Err(xous_kernel::Error::ProcessNotFound);
                    }
                    new.current_thread = new_tid as _;
                }
                ProcessState::Running(_) => {
                    panic!("process was running even though the pid was different")
                }
                #[cfg(feature = "gdb-stub")]
                ProcessState::Debug(_) | ProcessState::DebugIrq(_) => {
                    return Err(xous_kernel::Error::ProcessNotFound)
                }
                ProcessState::Sleeping | ProcessState::BlockedException(_) => {
                    // println!("PID {} was sleeping or being debugged", new_pid);
                    return Err(xous_kernel::Error::ProcessNotFound);
                }
            }

            // Perform the actual switch to the new memory space.  From this
            // point onward, we will need to activate the previous memory space
            // if we encounter an error.
            new.mapping.activate()?;

            // Set up the new process, if necessary.  Remove the new thread from
            // the list of ready threads.
            // let old_state = new.state;
            new.state = match new.state {
                ProcessState::Setup(thread_init) => {
                    // klog!("Setting up new process...");
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
                ProcessState::Exception(x) => ProcessState::Exception(x),
                ProcessState::BlockedException(_) => {
                    panic!("process was blocked handling an exception")
                }
                #[cfg(feature = "gdb-stub")]
                ProcessState::Debug(_) | ProcessState::DebugIrq(_) => {
                    panic!("process was being debugged")
                }
            };
            // log_process_update(file!(), line!(), new, old_state);
            new.activate()?;

            // Mark the previous process as ready to run, since we just switched
            // away
            let previous = self
                .get_process_mut(previous_pid)
                .expect("couldn't get previous pid");
            let _oldstate = previous.state; // for tracking state in the debug print after the following closure
            if previous.current_thread != previous_tid {
                println!(
                    "WARNING: previous.current_thread {} != previous_tid {}",
                    previous.current_thread, previous_tid
                );
            }
            previous.current_thread = previous_tid;
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
                    if can_resume {
                        ProcessState::Ready(x | (1 << previous_tid))
                    } else {
                        ProcessState::Ready(x)
                    }
                }
                ProcessState::Exception(ready_threads) => {
                    if can_resume {
                        ProcessState::Exception(ready_threads)
                    } else {
                        ProcessState::BlockedException(ready_threads)
                    }
                }
                other => panic!(
                    "previous process PID {} was in an invalid state (not Running): {:?}",
                    previous_pid, other
                ),
            };
            // log_process_update(file!(), line!(), previous, _oldstate);
            klog!(
                "PID {:?} state change from {:?} -> {:?}",
                previous_pid,
                _oldstate,
                previous.state
            );
            // klog!(
            //     "Set previous process PID {} state to {:?} (with can_resume = {})",
            //     previous_pid,
            //     previous.state,
            //     can_resume
            // );
        } else {
            let new = self.get_process_mut(new_pid)?;

            // If we wanted to switch to a "new" thread, and it's the same
            // as the one we just switched from, do nothing.
            if previous_tid == new_tid {
                if !can_resume {
                    panic!("tried to switch to our own thread without resume (current_thread: {}  previous_tid: {}  new_tid: {})",
                            new.current_thread, previous_tid, new_tid);
                }
                let mut process = ArchProcess::current();
                process.set_tid(new_tid).unwrap();
                new.current_thread = new_tid;
                return Ok(new_tid);
            }

            // Transition to the new state.
            // let old_state = new.state;
            new.state = if let ProcessState::Running(x) = new.state {
                assert!(x & (1 << new.current_thread) == 0);

                // If the current process can be resumed, add it to the list
                // of potential threads
                let x = x | if can_resume {
                    1 << new.current_thread
                } else {
                    0
                };

                // If no new thread is specified, take the previous
                // thread.  If that is not runnable, do a round-robin
                // search for the next available thread.
                if new_tid == 0 {
                    new_tid = Self::find_next_thread(x, new.current_thread);
                }

                if x & (1 << new_tid) == 0 {
                    return Err(xous_kernel::Error::ThreadNotAvailable);
                }

                new.current_thread = new_tid as _;

                // Remove the new TID from the list of threads that can be run.
                ProcessState::Running(x & !(1 << new_tid))
            } else {
                panic!(
                    "PID {} invalid process state (not Running): {:?}",
                    previous_pid, new.state
                )
            };
            // log_process_update(file!(), line!(), new, old_state);
        }

        // Restore the previous thread, if one exists.
        ArchProcess::current().set_tid(new_tid)?;

        klog!(
            "Activated process {}:{}, new state: {:?}",
            new_pid,
            new_tid,
            self.get_process_mut(new_pid)?.state
        );

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
        src_virt: *mut usize,
        dest_pid: PID,
        dest_virt: *mut usize,
        len: usize,
    ) -> Result<*mut usize, xous_kernel::Error> {
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
        if (dest_virt as usize) + len > crate::arch::mem::USER_AREA_END {
            return Err(xous_kernel::Error::BadAddress);
        }

        let current_pid = self.current_pid();

        // Iterators and `ptr.wrapping_add()` operate on `usize` types,
        // which effectively lowers the `len`.
        let usize_len = len / core::mem::size_of::<usize>();
        let usize_page = crate::mem::PAGE_SIZE / core::mem::size_of::<usize>();

        // If the dest and src PID is the same, do nothing.
        if current_pid == dest_pid {
            crate::mem::MemoryManager::with_mut(|mm| {
                for offset in (0..usize_len).step_by(usize_page) {
                    mm.ensure_page_exists(src_virt.wrapping_add(offset) as usize)?;
                }
                Ok(())
            })?;
            return Ok(src_virt);
        }

        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        crate::mem::MemoryManager::with_mut(|mm| {
            // Locate an address to fit the new memory.
            dest_mapping.activate()?;
            let dest_virt = mm
                .find_virtual_address(dest_virt as *mut u8, len, xous_kernel::MemoryType::Messages)
                .map_err(|e| {
                    src_mapping.activate().expect("couldn't undo mapping");
                    e
                })? as *mut usize;
            src_mapping
                .activate()
                .expect("Couldn't switch back to source mapping");

            let mut error = None;

            // Move each subsequent page.
            for offset in (0..usize_len).step_by(usize_page) {
                assert!(((src_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                assert!(((dest_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                mm.ensure_page_exists(src_virt.wrapping_add(offset) as usize)?;
                mm.move_page(
                    current_pid,
                    &src_mapping,
                    src_virt.wrapping_add(offset) as *mut u8,
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset) as *mut u8,
                )
                .unwrap_or_else(|e| error = Some(e));
            }
            error.map_or_else(|| Ok(dest_virt), |e| panic!("unable to send: {:?}", e))
        })
        .map(|val| val as *mut usize)
    }

    #[cfg(not(baremetal))]
    pub fn send_memory(
        &mut self,
        src_virt: *mut usize,
        _dest_pid: PID,
        _dest_virt: *mut usize,
        _len: usize,
    ) -> Result<*mut usize, xous_kernel::Error> {
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
        src_virt: *mut usize,
        dest_pid: PID,
        dest_virt: *mut usize,
        len: usize,
        mutable: bool,
    ) -> Result<*mut usize, xous_kernel::Error> {
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
        // Iterators and `ptr.wrapping_add()` operate on `usize` types,
        // which effectively lowers the `len`.
        let usize_len = len / core::mem::size_of::<usize>();
        let usize_page = crate::mem::PAGE_SIZE / core::mem::size_of::<usize>();

        let current_pid = self.current_pid();
        // If it's within the same process, ignore the move operation and
        // just ensure the pages actually exist.
        if current_pid == dest_pid {
            MemoryManager::with_mut(|mm| {
                for offset in (0..usize_len).step_by(usize_page) {
                    assert!(((src_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                    mm.ensure_page_exists(src_virt.wrapping_add(offset) as usize)?;
                }
                Ok(())
            })?;
            return Ok(src_virt);
        }
        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        use crate::mem::MemoryManager;
        MemoryManager::with_mut(|mm| {
            // Locate an address to fit the new memory.
            dest_mapping.activate()?;
            let dest_virt = mm
                .find_virtual_address(dest_virt as *mut u8, len, xous_kernel::MemoryType::Messages)
                .map_err(|e| {
                    src_mapping.activate().unwrap();
                    // klog!("Couldn't find a virtual address");
                    e
                })? as *mut usize;
            src_mapping.activate().unwrap();

            let mut error = None;

            // Lend each subsequent page.
            for offset in (0..usize_len).step_by(usize_page) {
                assert!(((src_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                assert!(((dest_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                mm.ensure_page_exists(src_virt.wrapping_add(offset) as usize)?;
                mm.lend_page(
                    &src_mapping,
                    src_virt.wrapping_add(offset) as *mut u8,
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset) as *mut u8,
                    mutable,
                )
                .unwrap_or_else(|e| {
                    error = Some(e);
                    // klog!(
                    //     "Couldn't lend page {:08x} -> {:08x}",
                    //     src_virt.wrapping_add(offset) as usize,
                    //     dest_virt.wrapping_add(offset) as usize
                    // );
                    0
                });
            }
            error.map_or_else(
                || Ok(dest_virt),
                |e| {
                    panic!(
                        "unable to lend {:08x} in pid {} to {:08x} in pid {}: {:?}",
                        src_virt as usize, current_pid, dest_virt as usize, dest_pid, e
                    )
                },
            )
        })
        .map(|val| val as *mut usize)
    }

    #[cfg(not(baremetal))]
    pub fn lend_memory(
        &mut self,
        src_virt: *mut usize,
        _dest_pid: PID,
        _dest_virt: *mut usize,
        _len: usize,
        _mutable: bool,
    ) -> Result<*mut usize, xous_kernel::Error> {
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
        src_virt: *mut usize,
        dest_pid: PID,
        _dest_tid: TID,
        dest_virt: *mut usize,
        len: usize,
    ) -> Result<*mut usize, xous_kernel::Error> {
        // klog!(
        //     "Returning from {}:{} to {}:{}",
        //     self.current_pid(),
        //     _src_tid,
        //     dest_pid,
        //     _dest_tid
        // );
        if len == 0 {
            // klog!("No len");
            return Err(xous_kernel::Error::BadAddress);
        }
        if len & 0xfff != 0 {
            // klog!("len not aligned");
            return Err(xous_kernel::Error::BadAddress);
        }
        if src_virt as usize & 0xfff != 0 {
            // klog!("Src virt not aligned");
            return Err(xous_kernel::Error::BadAddress);
        }
        if dest_virt as usize & 0xfff != 0 {
            // klog!("dest virt not aligned");
            return Err(xous_kernel::Error::BadAddress);
        }

        // If memory is getting returned to the kernel, then it is memory that was
        // borrowed but
        if dest_pid.get() == 1 {}

        // Iterators and `ptr.wrapping_add()` operate on `usize` types,
        // which effectively lowers the `len`.
        let usize_len = len / core::mem::size_of::<usize>();
        let usize_page = crate::mem::PAGE_SIZE / core::mem::size_of::<usize>();

        let current_pid = self.current_pid();
        // If it's within the same process, ignore the operation.
        if current_pid == dest_pid {
            return Ok(src_virt);
        }
        let src_mapping = self.get_process(current_pid)?.mapping;
        let dest_mapping = self.get_process(dest_pid)?.mapping;
        use crate::mem::MemoryManager;
        MemoryManager::with_mut(|mm| {
            let mut error = None;

            // Lend each subsequent page.
            for offset in (0..usize_len).step_by(usize_page) {
                assert!(((src_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                assert!(((dest_virt.wrapping_add(offset) as usize) & 0xfff) == 0);
                mm.unlend_page(
                    &src_mapping,
                    src_virt.wrapping_add(offset) as *mut u8,
                    dest_pid,
                    &dest_mapping,
                    dest_virt.wrapping_add(offset) as *mut u8,
                )
                .unwrap_or_else(|e| {
                    // panic!(
                    //     "Couldn't unlend {:08x} from {:08x}: {:?}",
                    //     src_virt.wrapping_add(offset) as usize,
                    //     dest_virt.wrapping_add(offset) as usize,
                    //     e
                    // );
                    error = Some(e);
                    0
                });
            }
            error.map_or_else(|| Ok(dest_virt), Err)
        })
        .map(|val| val as *mut usize)
    }

    #[cfg(not(baremetal))]
    pub fn return_memory(
        &mut self,
        src_virt: *mut usize,
        dest_pid: PID,
        dest_tid: TID,
        _dest_virt: *mut usize,
        len: usize,
        // buf: MemoryRange,
    ) -> Result<*mut usize, xous_kernel::Error> {
        let buf = unsafe { MemoryRange::new(src_virt as usize, len) }?;
        let buf = buf.as_slice();
        let current_pid = self.current_pid();
        {
            let target_process = self.get_process(dest_pid)?;
            target_process.activate()?;
            let mut arch_process = ArchProcess::current();
            arch_process.return_memory(dest_tid, buf);
        }
        let target_process = self.get_process(current_pid)?;
        target_process.activate()?;

        Ok(src_virt as *mut usize)
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

        let mut arch_process = ArchProcess::current();
        let new_tid = arch_process
            .find_free_thread()
            .ok_or(xous_kernel::Error::ThreadNotAvailable)?;

        arch_process.setup_thread(new_tid, thread_init)?;

        // klog!("KERNEL({}): Created new thread {}", pid, new_tid);

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

    /// Destroy the given thread. Returns `true` if the PID has been updated.
    /// # Errors
    ///
    /// * **ThreadNotAvailable**: The thread does not exist in this process
    #[cfg(baremetal)]
    pub fn destroy_thread(&mut self, pid: PID, tid: TID) -> Result<bool, xous_kernel::Error> {
        let current_pid = self.current_pid();
        assert_eq!(pid, current_pid);

        let mut waiting_threads = match self.get_process_mut(pid)?.state {
            ProcessState::Running(x) => x,
            state => panic!("Process was in an invalid state: {:?}", state),
        };

        // Destroy the thread at a hardware level
        let mut arch_process = ArchProcess::current();
        let return_value = arch_process.destroy_thread(tid).unwrap_or_default();

        // If there's another thread waiting on the return value of this thread,
        // wake it up and set its return value.
        if let Some((waiting_tid, _thread)) = arch_process.find_thread(|waiting_tid, thr| {
            (waiting_threads & (1 << waiting_tid)) == 0 // Thread is waiting (i.e. not ready to run)
                && thr.a0() == (xous_kernel::SysCallNumber::JoinThread as usize) // Thread called `JoinThread`
                && thr.a1() == (tid as usize) // It is waiting on our thread
        }) {
            // Wake up the thread
            self.set_thread_result(pid, waiting_tid, xous_kernel::Result::Scalar1(return_value))?;
            waiting_threads |= 1 << waiting_tid;
        }

        // Mark this process as `Ready` if there are waiting threads, or `Sleeping` if
        // there are no waiting threads.
        let mut new_pid = pid;
        {
            let process = self.get_process_mut(pid)?;
            // let old_state = process.state;
            process.state = if waiting_threads == 0 {
                new_pid = process.ppid;
                ProcessState::Sleeping
            } else {
                ProcessState::Ready(waiting_threads)
            };
            // log_process_update(file!(), line!(), process, old_state);
        }

        // Switch to the next available TID. This moves the process back to a `Running` state.
        self.switch_to_thread(new_pid, None)?;

        Ok(new_pid != pid)
    }

    /// Park this thread if the target thread is currently running. Otherwise,
    /// return the value of the given thread.
    pub fn join_thread(
        &mut self,
        pid: PID,
        tid: TID,
        join_tid: TID,
    ) -> Result<xous_kernel::Result, xous_kernel::Error> {
        let current_pid = self.current_pid();
        assert_eq!(pid, current_pid);

        // We cannot wait on ourselves.
        if tid == join_tid {
            return Err(xous_kernel::Error::ThreadNotAvailable);
        }

        // If the target thread exists, put this thread to sleep.
        let arch_process = ArchProcess::current();
        if arch_process.thread_exists(join_tid) {
            // The target thread exists -- put this thread to sleep
            let ppid = self.get_process(pid).unwrap().ppid;
            self.activate_process_thread(tid, ppid, 0, false)
                .map(|_| Ok(xous_kernel::Result::ResumeProcess))
                .unwrap_or(Err(xous_kernel::Error::ProcessNotFound))
        } else {
            // The thread does not exist -- continue execution
            // Err(xous_kernel::Error::ThreadNotAvailable)
            Ok(xous_kernel::Result::Scalar1(0))
        }
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
        connect: bool,
    ) -> Result<(SID, CID), xous_kernel::Error> {
        // klog!(
        //     "looking through server list for free server, connect? {}",
        //     connect
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
            if *entry == None {
                #[cfg(baremetal)]
                // Allocate a single page for the server queue
                let backing = crate::mem::MemoryManager::with_mut(|mm| unsafe {
                    MemoryRange::new(
                        mm.map_zeroed_page(pid, false)? as _,
                        crate::arch::mem::PAGE_SIZE,
                    )
                })?;

                #[cfg(not(baremetal))]
                let backing = unsafe { MemoryRange::new(4096, 4096).unwrap() };

                // klog!("initializing new server with backing at {:x?} -- entry is {:?} (connect? {:?})", backing, *entry, connect);
                // Initialize the server with the given memory page.
                Server::init(entry, pid, sid, backing).unwrap();

                let cid = if connect {
                    self.connect_to_server(sid)?
                } else {
                    0
                };
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
        connect: bool,
    ) -> Result<(SID, CID), xous_kernel::Error> {
        let sid = self.create_server_id()?;
        self.create_server_with_address(pid, sid, connect)
    }

    /// Generate a random server ID and return it to the caller. Doesn't create
    /// any processes.
    pub fn create_server_id(&mut self) -> Result<SID, xous_kernel::Error> {
        let sid = SID::from_u32(
            platform::rand::get_u32(),
            platform::rand::get_u32(),
            platform::rand::get_u32(),
            platform::rand::get_u32(),
        );
        Ok(sid)
    }

    /// Destroy the provided server ID and disconnect any processes that are
    /// connected.
    pub fn destroy_server(&mut self, pid: PID, sid: SID) -> Result<(), xous_kernel::Error> {
        let mut idx_to_destroy = None;
        // Look through the server list for a server that matches this SID
        for (idx, entry) in self.servers.iter().enumerate() {
            if let Some(server) = entry {
                if server.sid == sid && server.pid == pid {
                    idx_to_destroy = Some(idx);
                    break;
                }
            }
        }

        let server_idx = idx_to_destroy.ok_or(xous_kernel::Error::ServerNotFound)?;
        let server = self.servers[server_idx].take().unwrap();
        // Try to destroy the server. This will fail if the server
        // has any outstanding memory requests.
        server.destroy(self).map_err(|server| {
            self.servers[server_idx] = Some(server);
            xous_kernel::Error::ServerQueueFull
        })?;

        let pid = crate::arch::process::current_pid();
        // println!("KERNEL({}): Server table: {:?}", _pid.get(), self.servers);
        // Disconnect this server from all processes.
        for process in self.processes.iter_mut() {
            if !process.free() {
                process.activate().unwrap();
                ArchProcess::with_inner_mut(|process_inner| {
                    // Look through the connection map for (1) a free slot, and (2) an
                    // existing connection
                    #[allow(clippy::manual_flatten)]
                    for server_idx_opt in process_inner.connection_map.iter_mut() {
                        if let Some(client_server_idx) = server_idx_opt {
                            if client_server_idx.get() == (server_idx + 2) as _ {
                                *server_idx_opt = None;
                                continue;
                            }
                        }
                    }
                });
            }
        }

        // Switch back to the primary process.
        self.get_process(pid).unwrap().activate().unwrap();
        Ok(())
    }

    /// Connect to a server on behalf of another process.
    pub fn connect_process_to_server(
        &mut self,
        target_pid: PID,
        sid: SID,
    ) -> Result<CID, xous_kernel::Error> {
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
        ArchProcess::with_inner_mut(|process_inner| {
            assert_eq!(pid, process_inner.pid);
            let mut slot_idx = None;
            // Look through the connection map for (1) a free slot, and (2) an
            // existing connection
            for (connection_idx, server_idx) in process_inner.connection_map.iter().enumerate() {
                // If we find an empty slot, use it
                let Some(server_idx) = server_idx else {
                    if slot_idx.is_none() {
                        slot_idx = Some(connection_idx);
                    }
                    continue;
                };
                let server_idx = server_idx.get() as usize;

                // Tombstone or unallocated server index
                if server_idx < 2 {
                    continue;
                }

                // If a connection to this server ID exists already, return it.
                let server_idx = server_idx - 2;
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
            let slot_idx = slot_idx.ok_or(Error::OutOfMemory)?;

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

    /// Invalidate the provided connection ID.
    pub fn disconnect_from_server(&mut self, cid: CID) -> Result<(), xous_kernel::Error> {
        // Check to see if we've already connected to this server.
        // While doing this, find a free slot in case we haven't
        // yet connected.

        // Slot indices are offset by two. Ensure we don't underflow.
        let slot_idx = cid;
        if slot_idx < 2 {
            klog!("CID {} is not valid", cid);
            return Err(xous_kernel::Error::ServerNotFound);
        }
        let slot_idx = (slot_idx - 2) as usize;
        let pid = crate::arch::process::current_pid();
        // klog!("KERNEL({}): Server table: {:?}", pid.get(), self.servers);
        ArchProcess::with_inner_mut(|process_inner| {
            assert_eq!(pid, process_inner.pid);
            if slot_idx >= process_inner.connection_map.len() {
                klog!("Slot index exceeds map length");
                return Err(xous_kernel::Error::ServerNotFound);
            }

            // If the server ID is None, then we weren't connected in the first place.
            let idx = &mut process_inner.connection_map[slot_idx];
            if idx.is_none() {
                klog!("IDX[{}] is already None!", slot_idx);
                return Err(xous_kernel::Error::ServerNotFound);
            }

            // Nullify this connection ID. It may now be reused.
            *idx = None;
            klog!("Removing server from table");
            Ok(())
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
        if cid < 2 {
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
            let connection_value = *process_inner.connection_map.get(cid as usize)?;
            let mut server_idx = connection_value?.get() as usize;
            if server_idx < 2 {
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
        thread: TID,
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
            server.queue_message(pid, thread, message, original_address)
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
                            for mapping in process_inner.connection_map.iter_mut().flatten() {
                                if mapping.get() == (idx as u8) + 2 {
                                    *mapping = NonZeroU8::new(1).unwrap();
                                }
                            }
                        })
                    }
                }

                let process = self.processes[(server.pid.get() - 1) as usize];
                process.activate().unwrap();
                // Look through this server's memory space to determine if this process
                // is mentioned there as having some memory lent out.
                server.discard_messages_for_pid(target_pid);
            }
        }

        // Now that the server has been "Disconnected", free the server entry.
        #[allow(clippy::manual_flatten)]
        for server in self.servers.iter_mut() {
            if let Some(server_inner) = server {
                if server_inner.pid == target_pid {
                    *server = None;
                }
            }
        }

        let process = self.get_process_mut(target_pid)?;
        process.activate()?;
        let parent_pid = process.ppid;
        process.terminate()?;

        self.switch_to_thread(parent_pid, None).unwrap();

        Ok(parent_pid)
    }

    #[cfg(feature = "gdb-stub")]
    pub fn pause_process_for_debug(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        println!("Pausing process {:?} for debug...", pid);
        let (process_state, parent_pid) = {
            let process = self.get_process_mut(pid)?;
            (process.state, process.ppid)
        };

        // Disable all interrupts that belong to this process
        crate::irq::for_each_irq(|irq_no, irq_pid, _, _| {
            println!("Examining IRQ {}...", irq_no);
            if pid == *irq_pid {
                println!(
                    "Disabling IRQ {} since it's owned by process {:?}",
                    irq_no, pid
                );
                crate::arch::irq::disable_irq(irq_no);
            }
        });
        let new_process_state = match process_state {
            ProcessState::Allocated => ProcessState::Allocated,
            ProcessState::Free => ProcessState::Free,
            ProcessState::Setup(thread_init) => ProcessState::Setup(thread_init),
            ProcessState::Ready(tids) => ProcessState::Debug(tids),
            ProcessState::Exception(tids) => ProcessState::Exception(tids),
            ProcessState::BlockedException(tids) => ProcessState::BlockedException(tids),
            ProcessState::Sleeping => ProcessState::Debug(0),
            ProcessState::Debug(tids) => ProcessState::Debug(tids),
            ProcessState::DebugIrq(tids) => ProcessState::DebugIrq(tids),
            ProcessState::Running(tids) => {
                // Switch to the parent process when we return.
                let current_tid = arch::process::current_tid();

                let parent_process = self.get_process_mut(parent_pid).unwrap();
                parent_process.activate().unwrap();
                let mut p = ArchProcess::current();
                // FIXME: What happens if this fails? We're currently in the new process
                // but without a context to switch to.
                p.set_tid(parent_process.previous_thread).unwrap();
                parent_process.current_thread = parent_process.previous_thread;
                parent_process.state = match parent_process.state {
                    ProcessState::Ready(x) | ProcessState::Running(x)
                        if x & (1 << parent_process.previous_thread) != 0 =>
                    {
                        ProcessState::Running(x & !(1 << parent_process.previous_thread))
                    }
                    ProcessState::Sleeping => ProcessState::Running(0),
                    _ => panic!(
                        "parent process was not ready to be resumed: {:?}",
                        parent_process.state
                    ),
                };

                // Ensure we don't switch back to the same process.
                unsafe { crate::arch::irq::take_isr_return_pair() };
                ProcessState::Debug(tids | 1 << current_tid)
            }
        };
        {
            let process = self.get_process_mut(pid).unwrap();
            println!(
                "Process {:?} went from {:?} to {:?}",
                pid, process.state, new_process_state
            );
            // let old_state = process.state;
            process.state = new_process_state;
            // log_process_update(file!(), line!(), process, old_state);
        }
        // self.get_process_mut(pid).unwrap().state = new_process_state;
        Ok(())
    }

    #[cfg(feature = "gdb-stub")]
    pub fn resume_process_from_debug(&mut self, pid: PID) -> Result<(), xous_kernel::Error> {
        let process = self.get_process_mut(pid)?;
        let old_state = process.state;
        process.state = match process.state {
            ProcessState::Allocated => ProcessState::Allocated,
            ProcessState::Free => ProcessState::Free,
            ProcessState::Setup(thread_init) => ProcessState::Setup(thread_init),
            ProcessState::Ready(tids) => ProcessState::Ready(tids),
            ProcessState::Sleeping => ProcessState::Sleeping,
            ProcessState::Exception(x) => ProcessState::Exception(x),
            ProcessState::BlockedException(tids) => ProcessState::BlockedException(tids),
            ProcessState::DebugIrq(tids) => ProcessState::Ready(tids),
            ProcessState::Debug(0) => ProcessState::Sleeping,
            ProcessState::Debug(tids) => ProcessState::Ready(tids),
            ProcessState::Running(tids) => ProcessState::Running(tids),
        };
        println!(
            "Resuming process {:?} from debug, going from {:?} to {:?}",
            pid, old_state, process.state
        );

        // Resume all interrupts that belong to this process
        crate::irq::for_each_irq(|irq, irq_pid, _, _| {
            if pid == *irq_pid {
                crate::arch::irq::enable_irq(irq);
            }
        });
        Ok(())
    }

    /// Calls the provided function with the current inner process state.
    pub fn shutdown(&mut self) -> Result<(), xous_kernel::Error> {
        // Destroy all servers. This will cause all queued messages to be lost.
        for server_idx in 0..self.servers.len() {
            if let Some(server) = self.servers[server_idx].take() {
                server.destroy(self).unwrap();
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
    /* https://github.com/betrusted-io/xous-core/issues/90
    /// Sets the exception handler for the given process ID. If an exception handler
    /// exists, it will be silently overridden.
    pub fn set_exception_handler(
        &mut self,
        pid: PID,
        pc: usize,
        sp: usize,
    ) -> Result<(), xous_kernel::Error> {
        self.get_process_mut(pid)?.exception_handler = if pc != 0 && sp != 0 {
            Some(ExceptionHandler { pc, sp })
        } else {
            None
        };
        Ok(())
    }
    */

    /// Causes the provided process to go into an exception state. This will fail
    /// if any of the following are true:
    ///     1. The process does not exist
    ///     2. The process has no exception handler
    ///     3. The process is not "Running" or "Ready"
    #[cfg(baremetal)]
    #[allow(dead_code)]
    pub fn begin_exception_handler(&mut self, pid: PID) -> Option<ExceptionHandler> {
        let process = self.get_process_mut(pid).ok()?;
        let handler = process.exception_handler?;
        process.state = match process.state {
            ProcessState::Running(x) => ProcessState::Exception(x | 1 << process.current_thread),
            ProcessState::Ready(x) => ProcessState::Exception(x),
            _ => return None,
        };
        process.previous_thread = process.current_thread;
        process.current_thread = crate::arch::process::EXCEPTION_TID;
        // Activate the current context
        let mut arch_process = ArchProcess::current();
        arch_process
            .set_tid(crate::arch::process::EXCEPTION_TID)
            .ok()?;
        Some(handler)
    }

    /// Move the current process from an `Exception` state back into a `Running` state
    /// with the current thread being marked as the given tid.
    #[cfg(baremetal)]
    pub fn finish_exception_handler_and_resume(
        &mut self,
        pid: PID,
    ) -> Result<(), xous_kernel::Result> {
        let process = self.get_process_mut(pid)?;
        if let ProcessState::Exception(threads) = process.state {
            assert!(threads & (1 << process.previous_thread) != 0);
            process.state = ProcessState::Running(threads & !(1 << process.previous_thread));
            process.current_thread = process.previous_thread;
            ArchProcess::current().set_tid(process.current_thread)?;
        } else {
            return Err(xous_kernel::Error::ThreadNotAvailable.into());
        }
        Ok(())
    }

    /// Returns the process name, if any, of a given PID
    #[cfg(baremetal)]
    pub fn process_name(&self, pid: PID) -> Option<&str> {
        let args = crate::args::KernelArguments::get();
        for arg in args.iter() {
            if arg.name != u32::from_le_bytes(*b"PNam") {
                continue;
            }
            let data = unsafe {
                let ptr = arg.data.as_ptr();
                let len = arg.size;
                core::slice::from_raw_parts(ptr as *const u8, len * 4)
            };
            let mut offset = 0;
            while offset <= arg.size {
                let check_pid = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                let str_len = u32::from_le_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]) as usize;
                if check_pid == pid.get() as _ {
                    if let Ok(s) = core::str::from_utf8(&data[offset + 8..offset + 8 + str_len]) {
                        return Some(s);
                    } else {
                        return None;
                    }
                }
                offset += str_len + 8;
                offset += (4 - (offset & 3)) & 3;
            }
        }
        None
    }
}
