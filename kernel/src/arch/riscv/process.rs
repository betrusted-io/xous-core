use core::mem;
static mut PROCESS: *mut ProcessImpl = 0xff80_1000 as *mut ProcessImpl;
pub const MAX_THREAD: TID = 31;
pub const INITIAL_TID: TID = 1;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous::{ProcessInit, ThreadInit, PID, TID};

use crate::args::KernelArguments;
pub const DEFAULT_STACK_SIZE: usize = 131072;
pub const MAX_PROCESS_COUNT: usize = 32;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_3000;
pub const IRQ_CONTEXT: usize = 1;

#[derive(Debug, Copy, Clone)]
struct ProcessImpl {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    threads: [Thread; MAX_THREAD + 1],

    /// The currently-active thread for this process
    current_thread: TID,
}

/// Singleton process table. Each process in the system gets allocated from this table.
struct ProcessTable {
    /// The process upon which the current syscall is operating
    current: PID,

    /// The number of processes that exist
    total: usize,

    /// The actual table contents
    table: [Option<ProcessImpl>; MAX_PROCESS_COUNT],
}

static mut PROCESS_TABLE: ProcessTable = ProcessTable {
    current: unsafe { PID::new_unchecked(1) },
    total: 0,
    table: [None; MAX_PROCESS_COUNT],
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
        Process {
            pid: unsafe { PROCESS_TABLE.current },
        }
    }

    /// Mark this process as running on the current core
    pub fn activate(&mut self) -> Result<(), xous::Error> {
        Ok(())
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_inner<F, R>(f: F) -> R
    where
        F: FnOnce(&ProcessInner) -> R,
    {
        todo!();
        // PROCESS_TABLE.with(|pt| {
        //     let process_table = pt.borrow();
        //     let current = &process_table.table[process_table.current.get() as usize - 1]
        //         .as_ref()
        //         .unwrap();
        //     f(&current.inner)
        // })
    }

    /// Calls the provided function with the current inner process state.
    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&Process) -> R,
    {
        let process = Self::current();
        f(&process)
    }

    pub fn with_inner_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessInner) -> R,
    {
        todo!();
        // PROCESS_TABLE.with(|pt| {
        //     let mut process_table = pt.borrow_mut();
        //     let current_pid_idx = process_table.current.get() as usize - 1;
        //     let current = &mut process_table.table[current_pid_idx].as_mut().unwrap();
        //     f(&mut current.inner)
        // })
    }

    pub fn current_thread(&mut self) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(process.current_thread != 0, "thread number was 0");
        &mut process.threads[process.current_thread - 1]
    }

    pub fn current_tid(&self) -> TID {
        let process = unsafe { &*PROCESS };
        process.current_thread
    }

    /// Set the current thread number.
    pub fn set_thread(&mut self, thread: TID) -> Result<(), xous::Error> {
        let mut process = unsafe { &mut *PROCESS };
        assert!(
            thread > 0 && thread <= process.threads.len(),
            "attempt to switch to an invalid thread {}",
            thread
        );
        process.current_thread = thread;
        Ok(())
    }

    pub fn thread(&mut self, thread: TID) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(
            thread > 0 && thread <= process.threads.len(),
            "attempt to retrieve an invalid thread {}",
            thread
        );
        &mut process.threads[thread - 1]
    }

    pub fn find_free_thread(&self) -> Option<TID> {
        let process = unsafe { &mut *PROCESS };
        for (index, thread) in process.threads.iter().enumerate() {
            if index != 0 && thread.sepc == 0 {
                return Some(index as TID + 1);
            }
        }
        None
    }

    pub fn set_thread_result(&mut self, thread_nr: TID, result: xous::Result) {
        let vals = unsafe { mem::transmute::<_, [usize; 8]>(result) };
        let thread = self.thread(thread_nr);
        for (idx, reg) in vals.iter().enumerate() {
            thread.registers[9 + idx] = *reg;
        }
    }

    /// Initialize this process thread with the given entrypoint and stack
    /// addresses.
    pub fn init(&mut self, entrypoint: usize, stack: usize, thread: usize) {
        let mut process = unsafe { &mut *PROCESS };
        assert!(
            mem::size_of::<Process>() == PAGE_SIZE,
            "Process size is {}, not PAGE_SIZE ({})",
            mem::size_of::<Process>(),
            PAGE_SIZE
        );
        assert!(
            thread + 1 < process.threads.len(),
            "tried to init a thread that's out of range"
        );
        assert!(thread != 1, "tried to init using the irq thread");
        assert!(thread != 0, "tried to init using a thread of 0");
        assert!(
            thread == 2,
            "tried to init using a thread that wasn't 2. This probably isn't what you want."
        );
        // By convention, thread 0 is the trap thread. Therefore, thread 1 is
        // the first default thread.
        process.current_thread = thread;
        for thread in process.threads.iter_mut() {
            *thread = Default::default();
        }

        let mut thread = self.current_thread();

        thread.registers = Default::default();
        thread.sepc = entrypoint;
        thread.registers[1] = stack;

        process.inner = Default::default();
    }

    pub fn setup_thread(&mut self, thread: TID, _setup: ThreadInit) -> Result<(), xous::Error> {
        todo!();
    }

    pub fn create(pid: PID, init_data: ProcessInit) -> PID {
        todo!();
    }

    pub fn destroy(pid: PID) -> Result<(), xous::Error> {
        todo!();
        // let mut process_table = unsafe { &mut *PROCESS };
        // let pid_idx = pid.get() as usize - 1;
        // if pid_idx >= process_table.table.len() {
        //     panic!("attempted to destroy PID that exceeds table index: {}", pid);
        // }
        // let process = process_table.table[pid_idx].as_mut().unwrap();
        // process_table.table[pid_idx] = None;
        // process_table.total -= 1;
        // Ok(())
    }
}

impl Thread {
    /// The current stack pointer for this thread
    pub fn stack_pointer(&self) -> usize {
        self.registers[1]
    }
}

pub fn set_current_pid(pid: PID) {
    let pid_idx = (pid.get() - 1) as usize;
    unsafe {
        let mut pt = &mut PROCESS_TABLE;

        match pt.table.get_mut(pid_idx) {
            None | Some(None) => {
                panic!("PID {} does not exist", pid);
            }
            Some(_) => {}
        }
        pt.current = pid;
    }
}

pub fn current_pid() -> PID {
    unsafe { PROCESS_TABLE.current }
}

pub fn current_tid() -> TID {
    unsafe { (*PROCESS).current_thread }
}

// pub struct ProcessHandle<'a> {
//     process: &'a mut Process,
// }

// /// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
// /// to the Memory Manager should only be made during interrupt contexts.
// impl<'a> ProcessHandle<'a> {
//     /// Get the singleton Process.
//     pub fn get() -> ProcessHandle<'a> {
//         ProcessHandle {
//             process: unsafe { &mut *PROCESS },
//         }
//     }
// }

// use core::ops::{Deref, DerefMut};
// impl Deref for ProcessHandle<'_> {
//     type Target = Process;
//     fn deref(&self) -> &Process {
//         &*self.process
//     }
// }
// impl DerefMut for ProcessHandle<'_> {
//     fn deref_mut(&mut self) -> &mut Process {
//         &mut *self.process
//     }
// }
