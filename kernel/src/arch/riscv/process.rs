use core::mem;
static mut PROCESS: *mut Process = 0xff80_1000 as *mut Process;
pub const MAX_CONTEXT: CtxID = 31;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous::CtxID;

#[repr(C)]
#[derive(Debug)]
pub struct Process {
    /// Used by the interrupt handler to calculate offsets
    scratch: usize,

    /// The index into the `contexts` list.  This must never be 0. The interrupt
    /// handler writes to this field, so it must not be moved.
    context_nr: CtxID,

    _hash: [usize; 8],

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The interrupt handler will save the current process to this Context when
    /// the trap handler is entered.
    contexts: [ProcessContext; MAX_CONTEXT],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct ProcessContext {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this context was created because of an
    /// `ecall` instruction, you will need to add `4` to this before returning,
    /// to prevent that instruction from getting executed again. If this is 0,
    /// then this context is not valid.
    pub sepc: usize,
}

impl Process {
    pub fn current_context(&mut self) -> &mut ProcessContext {
        assert!(self.context_nr != 0, "context number was 0");
        &mut self.contexts[self.context_nr - 1]
    }

    /// Return the current context number. Context numbers are 0-indexed.
    pub fn current_context_nr(&self) -> CtxID {
        assert!(self.context_nr != 0, "context number was 0");
        self.context_nr
    }

    /// Set the current context number.
    pub fn set_context_nr(&mut self, context: CtxID) {
        assert!(
            context > 0 && context <= self.contexts.len(),
            "attempt to switch to an invalid context {}",
            context
        );
        self.context_nr = context;
    }

    pub fn context(&mut self, context_nr: CtxID) -> &mut ProcessContext {
        assert!(
            context_nr > 0 && context_nr <= self.contexts.len(),
            "attempt to retrieve an invalid context {}",
            context_nr
        );
        &mut self.contexts[context_nr - 1]
    }

    pub fn find_free_context_nr(&self) -> Option<CtxID> {
        for (index, context) in self.contexts.iter().enumerate() {
            if index != 0 && context.sepc == 0 {
                return Some(index as CtxID + 1);
            }
        }
        None
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
