// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::mem;

use xous_kernel::{PID, ProcessInit, ProcessStartup, TID, ThreadInit, arch::Arguments};

use crate::arch::mem::THREAD_CONTEXT_AREA;
use crate::mem::PAGE_SIZE;
use crate::services::ProcessInner;

static mut PROCESS: *mut ProcessImpl = THREAD_CONTEXT_AREA as *mut ProcessImpl;
pub const MAX_THREAD: TID = 31;
pub const EXCEPTION_TID: TID = 1;
pub const INITIAL_TID: TID = 2;
pub const IRQ_TID: TID = 0;

pub const DEFAULT_STACK_SIZE: usize = 128 * 1024;
pub const MAX_PROCESS_COUNT: usize = 64;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_5000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_6000;

/// This is the address a thread will return to when it finishes handling an exception.
pub const RETURN_FROM_EXCEPTION_HANDLER: usize = 0xff80_7000;

// ProcessImpl occupies a multiple of pages mapped to virtual address `0xff80_4000` (THREAD_CONTEXT_AREA).
// Each thread is 128 bytes (32 4-byte registers). The first "thread" does not exist,
// and instead is any bookkeeping information related to the process.
#[derive(Debug, Clone)]
#[repr(C)]
struct ProcessImpl {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The currently-active thread for this process. This must
    /// be the 2nd item, because the ISR directly accesses this value.
    hardware_thread: usize,

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The last thread ID that was allocated
    last_tid_allocated: u8,

    /// Pad everything to 128 bytes, so the Thread slice starts at
    /// offset 128.
    _padding: [u32; 13],

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    threads: [Thread; MAX_THREAD],
}

/// Singleton process table. Each process in the system gets allocated from this table.
struct ProcessTable {
    /// The process upon which the current syscall is operating
    current: PID,

    /// The actual table contents. `true` if a process is allocated,
    /// `false` if it is free.
    table: [bool; MAX_PROCESS_COUNT],
}

static mut PROCESS_TABLE: ProcessTable =
    ProcessTable { current: unsafe { PID::new_unchecked(1) }, table: [false; MAX_PROCESS_COUNT] };

pub fn set_current_pid(pid: PID) {
    let pid_idx = (pid.get() - 1) as usize;
    unsafe {
        let mut pt = &mut PROCESS_TABLE;

        match pt.table.get(pid_idx) {
            None | Some(false) => panic!("PID {} does not exist", pid),
            _ => (),
        }
        pt.current = pid;
    }
}

pub fn current_pid() -> PID { unsafe { PROCESS_TABLE.current } }

#[allow(dead_code)]
pub fn current_tid() -> TID { unsafe { ((*PROCESS).hardware_thread) - 1 } }

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Process {
    pid: PID,
}

impl Process {
    pub fn current() -> Process {
        let pid = unsafe { PROCESS_TABLE.current };
        // TODO: find a place where to call `set_hardware_pid()` for this to not panic
        //let hardware_pid = unsafe { get_hardware_pid() & 0xff }; // Discards the process ID field of
        // CONTEXTIDR assert_eq!((pid.get() as usize), hardware_pid,
        //           "Hardware current PID doesn't match the software. hw = {} vs sw = {}", pid,
        // hardware_pid);
        Process { pid }
    }

    pub fn activate(&mut self) -> Result<(), xous_kernel::Error> {
        let pid = self.pid.get() as usize;
        let pid_and_asid = (pid << 8) | pid; // Set both process ID and ASID
        unsafe {
            core::arch::asm!(
                "mcr p15, 0, {contextidr}, c13, c0, 1",
                contextidr = in(reg) pid_and_asid,
            )
        }

        Ok(())
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_inner<F, R>(f: F) -> R
    where
        F: FnOnce(&ProcessInner) -> R,
    {
        let process = unsafe { &*PROCESS };
        f(&process.inner)
    }

    /// Calls the provided function with the current inner process state.
    #[allow(dead_code)]
    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&Process) -> R,
    {
        let process = Self::current();
        f(&process)
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_current_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Process) -> R,
    {
        let mut process = Self::current();
        f(&mut process)
    }

    pub fn with_inner_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessInner) -> R,
    {
        let process = unsafe { &mut *PROCESS };
        f(&mut process.inner)
    }

    pub fn current_thread_mut(&mut self) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(process.hardware_thread != 0, "thread number was 0");
        &mut process.threads[process.hardware_thread - 1]
    }

    pub fn current_thread(&self) -> &Thread {
        let process = unsafe { &mut *PROCESS };
        &mut process.threads[process.hardware_thread - 1]
        // self.thread(process.hardware_thread - 1)
    }

    pub fn current_tid(&self) -> TID {
        let process = unsafe { &*PROCESS };
        process.hardware_thread - 1
    }

    pub fn thread_exists(&self, tid: TID) -> bool { self.thread(tid).resume_addr != 0 }

    /// Set the current thread number.
    pub fn set_tid(&mut self, thread: TID) -> Result<(), xous_kernel::Error> {
        let mut process = unsafe { &mut *PROCESS };
        klog!("Switching to thread {}", thread);
        assert!(thread <= process.threads.len(), "attempt to switch to an invalid thread {}", thread);
        process.hardware_thread = thread + 1;
        Ok(())
    }

    pub fn thread_mut(&mut self, thread: TID) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(thread <= process.threads.len(), "attempt to retrieve an invalid thread {}", thread);
        &mut process.threads[thread]
    }

    pub fn thread(&self, thread: TID) -> &Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(thread <= process.threads.len(), "attempt to retrieve an invalid thread {}", thread);
        &process.threads[thread]
    }

    #[cfg(feature = "gdb-stub")]
    pub fn for_each_thread_mut<F>(&self, mut op: F)
    where
        F: FnMut(TID, &Thread),
    {
        let process = unsafe { &mut *PROCESS };
        for (idx, thread) in process.threads.iter_mut().enumerate() {
            // Ignore threads that have no PC, and ignore the ISR thread
            if thread.resume_addr == 0 || idx == IRQ_TID {
                continue;
            }
            op(idx, thread);
        }
    }

    pub fn find_free_thread(&self) -> Option<TID> {
        let process = unsafe { &mut *PROCESS };
        let start_tid = process.last_tid_allocated as usize;
        let a = &process.threads[start_tid..process.threads.len()];
        let b = &process.threads[0..start_tid];
        for (index, thread) in a.iter().chain(b.iter()).enumerate() {
            let mut tid = index + start_tid;
            if tid >= process.threads.len() {
                tid -= process.threads.len()
            }

            if tid != IRQ_TID && tid != EXCEPTION_TID && thread.resume_addr == 0 {
                process.last_tid_allocated = tid as _;
                return Some(tid as TID);
            }
        }
        None
    }

    pub fn set_thread_result(&mut self, thread_nr: TID, result: xous_kernel::Result) {
        let thread = self.thread_mut(thread_nr);

        klog!("Setting TID={} result before: {:x?}", thread_nr, thread);

        // Thread context's r0 should hold a pointer to the syscall arguments/result structure
        // that's located on the thread's stack
        if thread.r0 <= thread.sp && thread.r0 > thread.sp - DEFAULT_STACK_SIZE {
            let args = thread.r0 as *mut Arguments;
            let args = unsafe { &mut *args };

            args.set_result(&result);
        } else {
            klog!(
                "r0 ({:08x}) is not within thread stack space: [{:08x}; {:08x}]",
                thread.r0,
                thread.sp - DEFAULT_STACK_SIZE,
                thread.sp
            );
        }

        klog!("Setting TID={} result before: {:x?}", thread_nr, thread);
    }

    pub fn retry_instruction(&mut self, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = unsafe { &mut *PROCESS };
        let mut thread = &mut process.threads[tid];
        if thread.resume_addr >= 4 {
            thread.resume_addr -= 4;
        }
        Ok(())
    }

    /// Initialize this process thread with the given entrypoint and stack
    /// addresses.
    pub fn setup_process(pid: PID, thread_init: ThreadInit) -> Result<(), xous_kernel::Error> {
        let mut process = unsafe { &mut *PROCESS };
        let tid = INITIAL_TID;

        if pid.get() > 1 {
            assert_eq!(pid, crate::arch::current_pid(), "hardware pid does not match setup pid");
        }

        assert!(tid != IRQ_TID, "tried to init using the irq thread");
        let size = mem::size_of::<ProcessImpl>();
        assert!(
            size == PAGE_SIZE,
            "Process size is {}, not PAGE_SIZE ({}) (Thread size: {}, array: {}, Inner: {})",
            mem::size_of::<ProcessImpl>(),
            PAGE_SIZE,
            mem::size_of::<Thread>(),
            mem::size_of::<[Thread; MAX_THREAD + 1]>(),
            mem::size_of::<ProcessInner>(),
        );
        assert!(tid - 1 < process.threads.len(), "tried to init a thread that's out of range");
        assert!(
            tid == INITIAL_TID,
            "tried to init using a thread {} that wasn't {}. This probably isn't what you want.",
            tid,
            INITIAL_TID
        );

        //klog!("Setting up new process {}", pid.get());
        unsafe {
            let pid_idx = (pid.get() as usize) - 1;
            assert!(!PROCESS_TABLE.table[pid_idx], "process {} is already allocated", pid);
            PROCESS_TABLE.table[pid_idx] = true;
        }

        // By convention, thread 0 is the trap thread. Therefore, thread 1 is
        // the first default thread. There is an offset of 1 due to how the
        // interrupt handler functions.
        process.hardware_thread = tid + 1;

        // Reset the thread state, since it's possibly uninitialized memory
        for thread in process.threads.iter_mut() {
            *thread = Default::default();
        }

        let mut thread = &mut process.threads[tid];

        thread.resume_addr = thread_init.call;
        thread.ret_addr = EXIT_THREAD;
        thread.sp = thread_init.stack.as_ptr() as usize + thread_init.stack.len();
        thread.r0 = thread_init.arg1;
        thread.r1 = thread_init.arg2;
        thread.r2 = thread_init.arg3;
        thread.r3 = thread_init.arg4;

        klog!("thread_init: {:x?}  thread: {:x?}", thread_init, thread);

        #[cfg(any(feature = "debug-print", feature = "print-panics"))]
        {
            let pid = pid.get();
            if pid != 1 {
                klog!(
                    "initializing PID {} thread {} with entrypoint {:08x}, stack @ {:08x}, arg {:08x}",
                    pid,
                    tid,
                    thread.resume_addr,
                    thread.sp,
                    thread.r0,
                );
            }
        }

        process.inner = Default::default();

        // Mark the stack as "unallocated-but-free"
        let init_sp = (thread_init.stack.as_ptr() as usize) & !0xfff;
        if init_sp != 0 {
            let stack_size = thread_init.stack.len();
            crate::mem::MemoryManager::with_mut(|memory_manager| {
                memory_manager
                    .reserve_range(
                        init_sp as *mut u8,
                        stack_size,
                        xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
                    )
                    .expect("couldn't reserve stack")
            });
        }
        Ok(())
    }

    pub fn setup_thread(&mut self, new_tid: TID, setup: ThreadInit) -> Result<(), xous_kernel::Error> {
        let entrypoint = unsafe { core::mem::transmute::<_, usize>(setup.call) };

        // Create the new context and set it to run in the new address space.
        let pid = self.pid.get();
        let thread = self.thread_mut(new_tid);

        //klog!("Setting up thread {}, pid {}", new_tid, pid);
        let sp = setup.stack.as_ptr() as usize + setup.stack.len();
        if sp <= 16 {
            return Err(xous_kernel::Error::BadAddress);
        }
        crate::arch::syscall::invoke(thread, pid == 1, entrypoint, (sp - 16) & !0xf, EXIT_THREAD, &[
            setup.arg1, setup.arg2, setup.arg3, setup.arg4,
        ]);
        Ok(())
    }

    /// Destroy a given thread and return its return value.
    ///
    /// # Returns
    ///     The return value of the function
    ///
    /// # Errors
    ///     xous::ThreadNotAvailable - the thread did not exist
    #[allow(dead_code)]
    pub fn destroy_thread(&mut self, _tid: TID) -> Result<usize, xous_kernel::Error> {
        todo!();
    }

    pub fn print_all_threads(&self) {
        let process = unsafe { &mut *PROCESS };
        for (tid_idx, &thread) in process.threads.iter().enumerate() {
            let tid = tid_idx + 1;
            if thread.sp != 0 {
                Self::print_thread(tid, &thread);
            }
        }
    }

    #[allow(dead_code)]
    pub fn print_current_thread(&self) {
        let thread = self.current_thread();
        let tid = self.current_tid();
        Self::print_thread(tid, thread);
    }

    pub fn print_thread(_tid: TID, _thread: &Thread) {
        println!("Thread {}:", _tid);
        println!(
            "\tPC: {:08x}   SP: {:08x}    TP: {:08x}    RA: {:08x}",
            _thread.pc, _thread.sp, _thread.tp, _thread.ret_addr,
        );
        println!(
            "\tR0: {:08x}   R1: {:08x}    R2: {:08x}    R3: {:08x}",
            _thread.r0, _thread.r1, _thread.r2, _thread.r3,
        );
        println!(
            "\tR4: {:08x}   R5: {:08x}    R6: {:08x}    R7: {:08x}",
            _thread.r4, _thread.r5, _thread.r6, _thread.r7,
        );
        println!(
            "\tR8: {:08x}   R9: {:08x}   R10: {:08x}   R11: {:08x}",
            _thread.r8, _thread.r9, _thread.r10, _thread.fp,
        );
        println!("\tIP: {:08x}   LR: {:08x}  SPSR: {:08x}", _thread.ip, _thread.lr, _thread.psr,);
    }

    pub fn create(
        _pid: PID,
        _init_data: ProcessInit,
        _services: &mut crate::SystemServices,
    ) -> Result<ProcessStartup, xous_kernel::Error> {
        todo!();
    }

    pub fn destroy(pid: PID) -> Result<(), xous_kernel::Error> {
        let mut process_table = unsafe { &mut PROCESS_TABLE };
        let pid_idx = pid.get() as usize - 1;
        if pid_idx >= process_table.table.len() {
            panic!("attempted to destroy PID that exceeds table index: {}", pid);
        }
        process_table.table[pid_idx] = false;
        Ok(())
    }

    pub fn find_thread<F>(&self, _op: F) -> Option<(TID, &mut Thread)>
    where
        F: Fn(TID, &Thread) -> bool,
    {
        todo!();
    }
}

/// Everything required to keep track of a single thread of execution.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Thread {
    pub r0: usize,  // 0
    pub r1: usize,  // 1
    pub r2: usize,  // 2
    pub r3: usize,  // 3
    pub r4: usize,  // 4
    pub r5: usize,  // 5
    pub r6: usize,  // 6
    pub r7: usize,  // 7
    pub r8: usize,  // 8
    pub r9: usize,  // 9
    pub r10: usize, // 10
    pub fp: usize,  // 11
    pub ip: usize,  // 12
    pub sp: usize,  // 13
    pub lr: usize,  // 14
    pub pc: usize,  // 15
    pub psr: usize, // 16

    /// A hardware "thread pointer" for TLS (see ARM ARM B3.12.46)
    pub tp: usize, // 17

    /// A return address when thread is resumed as an ISR
    pub ret_addr: usize, // 18

    /// An address to jump when resuming or invoking a thread/process to to avoid using LR for this purpose.
    pub resume_addr: usize, // 19

    _padding: [usize; 12],
}

// A compile-time check that the thread structure doesn't overflow
const _: () = {
    if mem::size_of::<Thread>() != (32 * 4) {
        panic!("Incorrect size of Thread structure. Ensure correct padding");
    }
};

impl Thread {
    /// The current stack pointer for this thread
    pub fn stack_pointer(&self) -> usize { self.sp }

    pub fn a0(&self) -> usize { self.r0 }

    pub fn a1(&self) -> usize { self.r1 }
}

#[repr(C)]
#[cfg(baremetal)]
#[derive(Debug, Copy, Clone)]
/// **Note**: this struct must be in sync with the loader version.
pub struct InitialProcess {
    /// Level-1 translation table base address of the process
    pub ttbr0: usize,

    /// Address Space ID (PID) of the process.
    pub asid: u8,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,
}

impl InitialProcess {
    pub fn pid(&self) -> PID { PID::new(self.asid).expect("non-zero PID") }
}
