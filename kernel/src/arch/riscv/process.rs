// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::mem;
static mut PROCESS: *mut ProcessImpl = 0xff80_1000 as *mut ProcessImpl;
pub const MAX_THREAD: TID = 31;
pub const INITIAL_TID: TID = 1;
pub const IRQ_TID: TID = 0;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous_kernel::{ProcessInit, ThreadInit, PID, TID};

// use crate::args::KernelArguments;
pub const DEFAULT_STACK_SIZE: usize = 131072;
pub const MAX_PROCESS_COUNT: usize = 64;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_3000;

// Thread IDs have three possible meaning:
// Logical Thread ID: What the user sees
// Thread Context Index: An index into the thread slice
// Hardware Thread ID: The index that the ISR uses
//
// The Hardware Thread ID is always equal to the Thread Context
// Index, minus one. For example, the default thread ID is
// Hardware Thread ID 1 is Thread Context Index 0.
// The Logical Thread ID is equal to the Hardware Thread ID
// plus one again. This is because the ISR context is Thread
// Context Index 0.
// Therefore, the first Logical Thread ID is 1, which maps
// to Hardware Thread ID 2, which is Thread Context Index 1.
//
// +-----------------+-----------------+-----------------+
// |    Thread ID    |  Context Index  | Hardware Thread |
// +=================+=================+=================+
// |   ISR Context   |        0        |        1        |
// |        1        |        1        |        2        |
// |        2        |        2        |        3        |

// ProcessImpl occupies a multiple of pages mapped to virtual address `0xff80_1000`.
// Each thread is 128 bytes (32 4-byte registers). The first "thread" does not exist,
// and instead is any bookkeeping information related to the process.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct ProcessImpl {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The currently-active thread for this process. This must
    /// be the 2nd item, because the ISR directly writes this value.
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

static mut PROCESS_TABLE: ProcessTable = ProcessTable {
    current: unsafe { PID::new_unchecked(1) },
    table: [false; MAX_PROCESS_COUNT],
};

#[repr(C)]
#[cfg(baremetal)]
/// The stage1 bootloader sets up some initial processes.  These are reported
/// to us as (satp, entrypoint, sp) tuples, which can be turned into a structure.
/// The first element is always the kernel.
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    pub satp: usize,

    /// Where execution begins
    pub entrypoint: usize,

    /// Address of the top of the stack
    pub sp: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct Process {
    pid: PID,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct Thread {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this thread was created because of an
    /// `ecall` instruction, you will need to add `4` to this before returning,
    /// to prevent that instruction from getting executed again. If this is 0,
    /// then this thread is not valid.
    pub sepc: usize,
}

impl Process {
    pub fn current() -> Process {
        let pid = unsafe { PROCESS_TABLE.current };
        let hardware_pid = (riscv::register::satp::read().bits() >> 22) & ((1 << 9) - 1);
        assert!((pid.get() as usize) == hardware_pid);
        Process { pid }
    }

    /// Mark this process as running on the current core
    pub fn activate(&mut self) -> Result<(), xous_kernel::Error> {
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

    pub fn thread_exists(&self, tid: TID) -> bool {
        self.thread(tid).sepc != 0
    }

    /// Set the current thread number.
    pub fn set_thread(&mut self, thread: TID) -> Result<(), xous_kernel::Error> {
        let mut process = unsafe { &mut *PROCESS };
        // println!("KERNEL({}:{}): Switching to thread {}", self.pid, process.hardware_thread - 1, thread);
        assert!(
            thread <= process.threads.len(),
            "attempt to switch to an invalid thread {}",
            thread
        );
        process.hardware_thread = thread + 1;
        Ok(())
    }

    pub fn thread_mut(&mut self, thread: TID) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(
            thread <= process.threads.len(),
            "attempt to retrieve an invalid thread {}",
            thread
        );
        &mut process.threads[thread]
    }

    pub fn thread(&self, thread: TID) -> &Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(
            thread <= process.threads.len(),
            "attempt to retrieve an invalid thread {}",
            thread
        );
        &process.threads[thread]
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

            if tid != IRQ_TID && thread.sepc == 0 {
                process.last_tid_allocated = tid as _;
                return Some(tid as TID);
            }
        }
        None
    }

    pub fn set_thread_result(&mut self, thread_nr: TID, result: xous_kernel::Result) {
        let vals = unsafe { mem::transmute::<_, [usize; 8]>(result) };
        let thread = self.thread_mut(thread_nr);
        for (idx, reg) in vals.iter().enumerate() {
            thread.registers[9 + idx] = *reg;
        }
    }

    pub fn retry_instruction(&mut self, tid: TID) -> Result<(), xous_kernel::Error> {
        let process = unsafe { &mut *PROCESS };
        let mut thread = &mut process.threads[tid];
        if thread.sepc >= 4 {
            thread.sepc -= 4;
        }
        Ok(())
    }

    /// Initialize this process thread with the given entrypoint and stack
    /// addresses.
    pub fn setup_process(pid: PID, thread_init: ThreadInit) -> Result<(), xous_kernel::Error> {
        let mut process = unsafe { &mut *PROCESS };
        let tid = INITIAL_TID;

        assert!(tid != IRQ_TID, "tried to init using the irq thread");
        assert!(
            mem::size_of::<ProcessImpl>() == PAGE_SIZE,
            "Process size is {}, not PAGE_SIZE ({}) (Thread size: {}, array: {}, Inner: {})",
            mem::size_of::<ProcessImpl>(),
            PAGE_SIZE,
            mem::size_of::<Thread>(),
            mem::size_of::<[Thread; MAX_THREAD + 1]>(),
            mem::size_of::<ProcessInner>(),
        );
        assert!(
            tid - 1 < process.threads.len(),
            "tried to init a thread that's out of range"
        );
        assert!(
            tid == INITIAL_TID,
            "tried to init using a thread {} that wasn't {}. This probably isn't what you want.",
            tid,
            INITIAL_TID
        );

        unsafe {
            let pid_idx = (pid.get() as usize) - 1;
            assert!(
                !PROCESS_TABLE.table[pid_idx],
                "process {} is already allocated",
                pid
            );
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

        let process = unsafe { &mut *PROCESS };
        let mut thread = &mut process.threads[tid];

        thread.sepc = unsafe { core::mem::transmute::<_, usize>(thread_init.call) };
        thread.registers[1] = thread_init.stack.as_ptr() as usize + thread_init.stack.len();
        thread.registers[9] = thread_init.arg1;
        thread.registers[10] = thread_init.arg2;
        thread.registers[11] = thread_init.arg3;
        thread.registers[12] = thread_init.arg4;

        #[cfg(any(feature = "debug-print", feature = "print-panics"))]
        {
            let pid = pid.get();
            if pid != 1 {
                klog!(
                    "initializing PID {} thread {} with entrypoint {:08x}, stack @ {:08x}, arg {:08x}",
                    pid, tid, thread.sepc, thread.registers[1], thread.registers[9],
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
                        stack_size + 4096,
                        xous_kernel::MemoryFlags::R | xous_kernel::MemoryFlags::W,
                    )
                    .expect("couldn't reserve stack")
            });
        }
        Ok(())
    }

    pub fn setup_thread(
        &mut self,
        new_tid: TID,
        setup: ThreadInit,
    ) -> Result<(), xous_kernel::Error> {
        let entrypoint = unsafe { core::mem::transmute::<_, usize>(setup.call) };
        // Create the new context and set it to run in the new address space.
        let pid = self.pid.get();
        let thread = self.thread_mut(new_tid);
        // println!("Setting up thread {}, pid {}", new_tid, pid);
        crate::arch::syscall::invoke(
            thread,
            pid == 1,
            entrypoint,
            setup.stack.as_ptr() as usize + setup.stack.len(),
            EXIT_THREAD,
            &[setup.arg1, setup.arg2, setup.arg3, setup.arg4],
        );
        Ok(())
    }

    /// Destroy a given thread and return its return value.
    ///
    /// # Returns
    ///     The return value of the function
    ///
    /// # Errors
    ///     xous::ThreadNotAvailable - the thread did not exist
    pub fn destroy_thread(&mut self, tid: TID) -> Result<usize, xous_kernel::Error> {
        let thread = self.thread_mut(tid);

        // Ensure this thread is valid
        if thread.sepc == 0 || tid == IRQ_TID {
            Err(xous_kernel::Error::ThreadNotAvailable)?;
        }

        // thread.registers[0] == x1
        // thread.registers[1] == x2
        // ...
        // thread.registers[4] == x5 == t0
        // ...
        // thread.registers[9] == x10 == a0
        // thread.registers[10] == x11 == a1
        let return_value = thread.registers[9];

        for val in &mut thread.registers {
            *val = 0;
        }
        thread.sepc = 0;

        Ok(return_value)
    }

    pub fn print_all_threads(&self) {
        let process = unsafe { &mut *PROCESS };
        &mut process.threads[process.hardware_thread - 1];
        for (tid_idx, &thread) in process.threads.iter().enumerate() {
            let tid = tid_idx + 1;
            if thread.registers[1] != 0 {
                Self::print_thread(tid, &thread);
            }
        }
    }

    pub fn print_current_thread(&self) {
        let thread = self.current_thread();
        let tid = self.current_tid();
        Self::print_thread(tid, &thread);
    }

    pub fn print_thread(_tid: TID, _thread: &Thread) {
        println!("Thread {}:", _tid);
        println!(
            "PC:{:08x}   SP:{:08x}   RA:{:08x}",
            _thread.sepc, _thread.registers[1], _thread.registers[0]
        );
        println!(
            "GP:{:08x}   TP:{:08x}",
            _thread.registers[2], _thread.registers[3]
        );
        println!(
            "T0:{:08x}   T1:{:08x}   T2:{:08x}",
            _thread.registers[4], _thread.registers[5], _thread.registers[6]
        );
        println!(
            "T3:{:08x}   T4:{:08x}   T5:{:08x}   T6:{:08x}",
            _thread.registers[27],
            _thread.registers[28],
            _thread.registers[29],
            _thread.registers[30]
        );
        println!(
            "S0:{:08x}   S1:{:08x}   S2:{:08x}   S3:{:08x}",
            _thread.registers[7],
            _thread.registers[8],
            _thread.registers[17],
            _thread.registers[18]
        );
        println!(
            "S4:{:08x}   S5:{:08x}   S6:{:08x}   S7:{:08x}",
            _thread.registers[19],
            _thread.registers[20],
            _thread.registers[21],
            _thread.registers[22]
        );
        println!(
            "S8:{:08x}   S9:{:08x}  S10:{:08x}  S11:{:08x}",
            _thread.registers[23],
            _thread.registers[24],
            _thread.registers[25],
            _thread.registers[26]
        );
        println!(
            "A0:{:08x}   A1:{:08x}   A2:{:08x}   A3:{:08x}",
            _thread.registers[9],
            _thread.registers[10],
            _thread.registers[11],
            _thread.registers[12]
        );
        println!(
            "A4:{:08x}   A5:{:08x}   A6:{:08x}   A7:{:08x}",
            _thread.registers[13],
            _thread.registers[14],
            _thread.registers[15],
            _thread.registers[16]
        );
    }

    pub fn create(_pid: PID, _init_data: ProcessInit) -> PID {
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

    pub fn find_thread<F>(&self, op: F) -> Option<(TID, &mut Thread)>
    where
        F: Fn(TID, &Thread) -> bool,
    {
        let process = unsafe { &mut *PROCESS };
        for (idx, thread) in process.threads.iter_mut().enumerate() {
            if thread.sepc == 0 {
                continue;
            }
            if op(idx, thread) {
                return Some((idx, thread));
            }
        }
        None
    }
}

impl Thread {
    /// The current stack pointer for this thread
    pub fn stack_pointer(&self) -> usize {
        self.registers[1]
    }

    pub fn a0(&self) -> usize {
        self.registers[9]
    }

    pub fn a1(&self) -> usize {
        self.registers[10]
    }
}

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

pub fn current_pid() -> PID {
    unsafe { PROCESS_TABLE.current }
}

pub fn current_tid() -> TID {
    unsafe { ((*PROCESS).hardware_thread) - 1 }
}
