use core::mem;
static mut PROCESS: *mut ProcessImpl = 0xff80_1000 as *mut ProcessImpl;
pub const MAX_THREAD: TID = 31;
pub const INITIAL_TID: TID = 1;
pub const IRQ_TID: usize = 0;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous::{ProcessInit, ThreadInit, PID, TID};

// use crate::args::KernelArguments;
pub const DEFAULT_STACK_SIZE: usize = 131072;
pub const MAX_PROCESS_COUNT: usize = 32;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
const EXIT_THREAD: usize = 0xff80_3000;

#[derive(Debug, Copy, Clone)]
#[repr(C)]
struct ProcessImpl {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The currently-active thread for this process
    current_thread: usize,

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// Pad everything to 128 bytes, so the Thread slice starts at
    /// offset 128.
    _padding: [u32; 14],

    /// This enables the kernel to keep track of threads in the
    /// target process, and know which threads are ready to
    /// receive messages.
    threads: [Thread; MAX_THREAD],
}

/// Singleton process table. Each process in the system gets allocated from this table.
struct ProcessTable {
    /// The process upon which the current syscall is operating
    current: PID,

    /// The number of processes that exist
    total: usize,

    /// The actual table contents
    table: [bool; MAX_PROCESS_COUNT],
}

static mut PROCESS_TABLE: ProcessTable = ProcessTable {
    current: unsafe { PID::new_unchecked(1) },
    total: 0,
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
        assert!(process.current_thread != 0, "thread number was 0");
        &mut process.threads[process.current_thread - 1]
    }

    pub fn current_thread(&self) -> &Thread {
        let process = unsafe { &mut *PROCESS };
        self.thread(process.current_thread)
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
        process.current_thread = thread + 1;
        Ok(())
    }

    pub fn thread_mut(&mut self, thread: TID) -> &mut Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(
            thread > 0 && thread <= process.threads.len(),
            "attempt to retrieve an invalid thread {}",
            thread
        );
        &mut process.threads[thread - 1]
    }

    pub fn thread(&self, thread: TID) -> &Thread {
        let process = unsafe { &mut *PROCESS };
        assert!(
            thread > 0 && thread <= process.threads.len(),
            "attempt to retrieve an invalid thread {}",
            thread
        );
        &process.threads[thread - 1]
    }

    pub fn find_free_thread(&self) -> Option<TID> {
        let process = unsafe { &mut *PROCESS };
        for (index, thread) in process.threads.iter().enumerate() {
            if index != IRQ_TID && thread.sepc == 0 {
                return Some(index as TID);
            }
        }
        None
    }

    pub fn set_thread_result(&mut self, thread_nr: TID, result: xous::Result) {
        let vals = unsafe { mem::transmute::<_, [usize; 8]>(result) };
        let thread = self.thread_mut(thread_nr);
        for (idx, reg) in vals.iter().enumerate() {
            thread.registers[9 + idx] = *reg;
        }
    }

    /// Initialize this process thread with the given entrypoint and stack
    /// addresses.
    pub fn setup_process(pid: PID, thread_init: ThreadInit) {
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
            tid + 1 < process.threads.len(),
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
        process.current_thread = tid + 1;

        // Reset the thread state, since it's possibly uninitialized memory
        for thread in process.threads.iter_mut() {
            *thread = Default::default();
        }

        let pid = pid.get();
        let process = unsafe { &mut *PROCESS };
        let mut thread = &mut process.threads[tid];

        thread.sepc = unsafe { core::mem::transmute::<_, usize>(thread_init.call) };
        thread.registers[1] = thread_init.stack.as_ptr() as usize + thread_init.stack.len();
        thread.registers[9] = thread_init.arg.map(|x| x.get()).unwrap_or_default();

        if pid != 1 {
            println!(
                "Initializing PID {} thread {} with entrypoint {:08x}, stack @ {:08x}, arg {:08x}",
                pid, tid, thread.sepc, thread.registers[1], thread.registers[9],
            );
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
                        xous::MemoryFlags::R | xous::MemoryFlags::W,
                    )
                    .expect("couldn't reserve stack")
            });
        }
    }

    pub fn setup_thread(&mut self, new_tid: TID, setup: ThreadInit) -> Result<(), xous::Error> {
        let entrypoint = unsafe { core::mem::transmute::<_, usize>(setup.call) };
        // Create the new context and set it to run in the new address space.
        let pid = self.pid.get();
        let thread = self.thread_mut(new_tid);
        println!("Setting up thread {}, pid {}", new_tid, pid);
        crate::arch::syscall::invoke(
            thread,
            pid == 1,
            entrypoint,
            setup.stack.as_ptr() as usize + setup.stack.len(),
            EXIT_THREAD,
            &[setup.arg.map(|x| x.get() as usize).unwrap_or_default()],
        );
        Ok(())
    }

    pub fn print_thread(&self) {
        let thread = self.current_thread();
        println!(
            "PC:{:08x}   SP:{:08x}   RA:{:08x}",
            thread.sepc, thread.registers[1], thread.registers[0]
        );
        println!(
            "GP:{:08x}   TP:{:08x}",
            thread.registers[2], thread.registers[3]
        );
        println!(
            "T0:{:08x}   T1:{:08x}   T2:{:08x}",
            thread.registers[4], thread.registers[5], thread.registers[6]
        );
        println!(
            "T3:{:08x}   T4:{:08x}   T5:{:08x}   T6:{:08x}",
            thread.registers[27], thread.registers[28], thread.registers[29], thread.registers[30]
        );
        println!(
            "S0:{:08x}   S1:{:08x}   S2:{:08x}   S3:{:08x}",
            thread.registers[7], thread.registers[8], thread.registers[17], thread.registers[18]
        );
        println!(
            "S4:{:08x}   S5:{:08x}   S6:{:08x}   S7:{:08x}",
            thread.registers[19], thread.registers[20], thread.registers[21], thread.registers[22]
        );
        println!(
            "S8:{:08x}   S9:{:08x}  S10:{:08x}  S11:{:08x}",
            thread.registers[23], thread.registers[24], thread.registers[25], thread.registers[26]
        );
        println!(
            "A0:{:08x}   A1:{:08x}   A2:{:08x}   A3:{:08x}",
            thread.registers[9], thread.registers[10], thread.registers[11], thread.registers[12]
        );
        println!(
            "A4:{:08x}   A5:{:08x}   A6:{:08x}   A7:{:08x}",
            thread.registers[13], thread.registers[14], thread.registers[15], thread.registers[16]
        );
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
    println!("ARCH: Setting current PID to {}", pid);
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
