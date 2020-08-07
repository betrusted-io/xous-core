use core::mem;
static mut PROCESS: *mut Process = 0xff80_1000 as *mut Process;
pub const MAX_THREAD: TID = 31;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous::{ProcessInit, ProcessKey, ThreadInit, PID, TID};

use crate::args::KernelArguments;
const DEFAULT_STACK_SIZE: usize = 131072;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_3000;
pub const IRQ_CONTEXT: usize = 1;


#[repr(C)]
#[cfg(baremetal)]
/// The stage1 bootloader sets up some initial processes.  These are reported
/// to us as (satp, entrypoint, sp) tuples, which can be turned into a structure.
/// The first element is always the kernel.
pub struct InitialProcess {
    /// The RISC-V SATP value, which includes the offset of the root page
    /// table plus the process ID.
    satp: usize,

    /// Where execution begins
    entrypoint: usize,

    /// Address of the top of the stack
    sp: usize,
}

#[repr(C)]
#[derive(Debug)]
pub struct Process {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The index into the `contexts` list.  This must never be 0. The interrupt
    /// handler writes to this field, so it must not be moved.
    context_nr: TID,

    _hash: [usize; 8],

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The interrupt handler will save the current process to this Thread when
    /// the trap handler is entered.
    contexts: [Thread; MAX_THREAD],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct Thread {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this context was created because of an
    /// `ecall` instruction, you will need to add `4` to this before returning,
    /// to prevent that instruction from getting executed again. If this is 0,
    /// then this context is not valid.
    pub sepc: usize,
}

impl Process {

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
        assert!(self.context_nr != 0, "context number was 0");
        &mut self.contexts[self.context_nr - 1]
    }

    /// Set the current context number.
    pub fn set_thread(&mut self, context: TID) {
        assert!(
            context > 0 && context <= self.contexts.len(),
            "attempt to switch to an invalid context {}",
            context
        );
        self.context_nr = context;
    }

    pub fn context(&mut self, context_nr: TID) -> &mut Thread {
        assert!(
            context_nr > 0 && context_nr <= self.contexts.len(),
            "attempt to retrieve an invalid context {}",
            context_nr
        );
        &mut self.contexts[context_nr - 1]
    }

    pub fn find_free_context_nr(&self) -> Option<TID> {
        for (index, context) in self.contexts.iter().enumerate() {
            if index != 0 && context.sepc == 0 {
                return Some(index as TID + 1);
            }
        }
        None
    }

    pub fn set_context_result(&mut self, context_nr: TID, result: xous::Result) {
        let vals = unsafe { mem::transmute::<_, [usize; 8]>(result) };
        let context = self.context(context_nr);
        for (idx, reg) in vals.iter().enumerate() {
            context.registers[9 + idx] = *reg;
        }
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn init(&mut self, entrypoint: usize, stack: usize, context: usize) {
        assert!(
            mem::size_of::<Process>() == PAGE_SIZE,
            "Process size is {}, not PAGE_SIZE ({})",
            mem::size_of::<Process>(),
            PAGE_SIZE
        );
        assert!(
            context + 1 < self.contexts.len(),
            "tried to init a context that's out of range"
        );
        assert!(context != 1, "tried to init using the irq context");
        assert!(context != 0, "tried to init using a context of 0");
        assert!(
            context == 2,
            "tried to init using a context that wasn't 2. This probably isn't what you want."
        );
        // By convention, context 0 is the trap context. Therefore, context 1 is
        // the first default context.
        self.context_nr = context;
        for context in self.contexts.iter_mut() {
            *context = Default::default();
        }

        let mut context = self.current_thread();

        context.registers = Default::default();
        context.sepc = entrypoint;
        context.registers[1] = stack;

        self.inner = Default::default();
    }
}

impl Thread {
    /// The current stack pointer for this context
    pub fn stack_pointer(&self) -> usize {
        self.registers[1]
    }
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
