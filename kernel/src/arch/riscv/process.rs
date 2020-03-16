use core::mem;
static mut PROCESS: *mut Process = 0xff80_1000 as *mut Process;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;

// use sha3::{Digest, Shake128};


#[repr(C)]
#[derive(Debug)]
pub struct Process {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The index into the `contexts` list.  This must never be 0.
    context_nr: usize,

    /// The previous context number prior to banking
    banked_context_nr: usize,

    _hash: [usize; 8],
    _padding: [usize; 14],

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The interrupt handler will save the current process to this Context
    /// when the trap handler is entered.
    contexts: [ProcessContext; 30],

    /// When a trap is handled, e.g. an IRQ, construct a new process.
    trap_context: ProcessContext,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct ProcessContext {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this context was created
    /// because of an `ecall` instruction, you will need to add `4`
    /// to this before returning, to prevent that instruction from
    /// getting executed again.
    pub sepc: usize,
}

impl Process {
    pub fn current_context(&mut self) -> &mut ProcessContext {
        // println!("current_context({:?}", self);
        &mut self.contexts[self.context_nr - 1]
    }

    pub fn trap_context(&mut self) -> &mut ProcessContext {
        &mut self.trap_context
    }

    /// Save the current context into a banked value, so we can handle
    /// an interrupt.
    pub fn bank(&mut self) {
        if self.banked_context_nr == 0 {
            self.banked_context_nr = self.context_nr;
            self.context_nr = 31;  // Points to `trap_context`
        }
    }

    /// Restore a banked context number and remove the previous bank.
    /// Used to resume after handling an IRQ.
    pub fn unbank(&mut self) {
        if self.banked_context_nr != 0 {
            self.context_nr = self.banked_context_nr;
            self.banked_context_nr = 0;
        }
    }

    /// Initialize this process context with the given entrypoint and stack
    /// addresses.
    pub fn init(&mut self, entrypoint: usize, stack: usize) {
        assert!(
            mem::size_of::<Process>() == PAGE_SIZE,
            "Process size is {}, not PAGE_SIZE ({})",
            mem::size_of::<Process>(),
            PAGE_SIZE
        );
        self.context_nr = 1;
        for context in self.contexts.iter_mut() {
            *context = Default::default();
        }
        self.trap_context = Default::default();

        let mut context = self.current_context();

        context.registers = Default::default();
        context.sepc = entrypoint;
        context.registers[1] = stack;

        self.inner = Default::default();
    }
}

impl ProcessContext {
    /// Determine whether a process context is valid.
    /// Contexts are valid when they have a place to return to --
    /// i.e. `SEPC` is nonzero
    pub fn valid(&self) -> bool {
        self.sepc != 0
    }

    /// Invalidate a context by removing its return address
    pub fn invalidate(&mut self) {
        self.sepc = 0;
    }

    /// The current stack pointer for this context
    pub fn stack_pointer(&self) -> usize {
        self.registers[1]
    }
}

pub struct ProcessHandle<'a> {
    process: &'a mut Process,
}

/// Wraps the MemoryManager in a safe mutex.  Because of this, accesses
/// to the Memory Manager should only be made during interrupt contexts.
impl<'a> ProcessHandle<'a> {
    /// Get the singleton Process.
    pub fn get() -> ProcessHandle<'a> {
        ProcessHandle {
            process: unsafe { &mut *PROCESS },
        }
    }
}

use core::ops::{Deref, DerefMut};
impl Deref for ProcessHandle<'_> {
    type Target = Process;
    fn deref(&self) -> &Process {
        &*self.process
    }
}
impl DerefMut for ProcessHandle<'_> {
    fn deref_mut(&mut self) -> &mut Process {
        &mut *self.process
    }
}
