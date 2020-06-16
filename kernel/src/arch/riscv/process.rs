use core::mem;
static mut PROCESS: *mut Process = 0xff80_1000 as *mut Process;
pub const MAX_CONTEXT: CtxID = 31;
use crate::arch::mem::PAGE_SIZE;
use crate::services::ProcessInner;
use xous;
use xous::CtxID;

use crate::args::KernelArguments;
const DEFAULT_STACK_SIZE: usize = 131072;
// pub use crate::arch::mem::DEFAULT_STACK_TOP;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_3000;
pub const IRQ_CONTEXT: usize = 1;

pub type ContextInit = (
    usize, /* entrypoint */
    usize, /* stack */
    usize, /* stack size */
);

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
    context_nr: CtxID,

    _hash: [usize; 8],

    /// Global parameters used by the operating system
    pub inner: ProcessInner,

    /// The interrupt handler will save the current process to this Context when
    /// the trap handler is entered.
    contexts: [Context; MAX_CONTEXT],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct Context {
    /// Storage for all RISC-V registers, minus $zero
    pub registers: [usize; 31],

    /// The return address.  Note that if this context was created because of an
    /// `ecall` instruction, you will need to add `4` to this before returning,
    /// to prevent that instruction from getting executed again. If this is 0,
    /// then this context is not valid.
    pub sepc: usize,
}

pub struct ContextInit {
    entrypoint: usize,
    stack: usize,
    context: usize,
}

impl Process {
    pub fn current_context(&mut self) -> &mut Context {
        assert!(self.context_nr != 0, "context number was 0");
        &mut self.contexts[self.context_nr - 1]
    }

    /// Set the current context number.
    pub fn set_context(&mut self, context: CtxID) {
        assert!(
            context > 0 && context <= self.contexts.len(),
            "attempt to switch to an invalid context {}",
            context
        );
        self.context_nr = context;
    }

    pub fn context(&mut self, context_nr: CtxID) -> &mut Context {
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

    pub fn set_context_result(&mut self, context_nr: CtxID, result: xous::Result) {
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

        let mut context = self.current_context();

        context.registers = Default::default();
        context.sepc = entrypoint;
        context.registers[1] = stack;

        self.inner = Default::default();
    }
}

impl Context {
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
