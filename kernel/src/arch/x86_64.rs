// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use xous::PID;

pub mod irq {
    /// Disable external interrupts
    pub fn disable_all_irqs() {
        unimplemented!();
    }

    /// Enable external interrupts
    pub fn enable_all_irqs() {
        unimplemented!();
    }

    pub fn enable_irq(irq_no: usize) {
        unimplemented!();
    }

    pub fn disable_irq(irq_no: usize) {
        unimplemented!();
    }
}

pub fn current_pid() -> PID {
    unimplemented!();
}

pub fn init() {}

pub mod syscall {
    use crate::arch::Context;
    pub fn invoke(
        context: &mut Context,
        supervisor: bool,
        pc: usize,
        sp: usize,
        ret_addr: usize,
        args: &[usize],
    ) -> ! {
        unimplemented!();
    }

    fn set_supervisor(supervisor: bool) {
        unimplemented!();
    }

    pub fn resume(supervisor: bool, context: &Context) -> ! {
        unimplemented!();
    }
}

pub mod mem {
    use crate::mem::MemoryManager;
    use xous::{Error, MemoryFlags, PID};
    #[derive(Copy, Clone, Default, PartialEq)]
    pub struct MemoryMapping {}
    impl MemoryMapping {
        pub unsafe fn from_raw(&mut self, new: usize) {
            unimplemented!();
        }
        pub fn get_pid(&self) -> PID {
            unimplemented!();
        }
        pub fn current() -> MemoryMapping {
            unimplemented!();
        }
        pub fn activate(&self) {
            unimplemented!();
        }
        pub fn flags_for_address(&self, addr: usize) -> usize {
            unimplemented!();
        }
        pub fn reserve_address(
            &mut self,
            mm: &mut MemoryManager,
            addr: usize,
            flags: MemoryFlags,
        ) -> Result<(), XousError> {
            unimplemented!();
        }
    }

    impl core::fmt::Debug for MemoryMapping {
        fn fmt(
            &self,
            fmt: &mut core::fmt::Formatter,
        ) -> core::result::Result<(), core::fmt::Error> {
            write!(fmt, "unimplemented",)
        }
    }

    pub fn map_page_inner(
        mm: &mut MemoryManager,
        pid: PID,
        phys: usize,
        virt: usize,
        req_flags: MemoryFlags,
    ) -> Result<(), XousError> {
        unimplemented!();
    }

    pub fn unmap_page_inner(mm: &mut MemoryManager, virt: usize) -> Result<usize, XousError> {
        unimplemented!();
    }

    pub const DEFAULT_MEMORY_MAPPING: MemoryMapping = MemoryMapping {};

    pub const DEFAULT_STACK_TOP: usize = 0xffff_0000;
    pub const DEFAULT_HEAP_BASE: usize = 0x4000_0000;
    pub const DEFAULT_MESSAGE_BASE: usize = 0x8000_0000;
    pub const DEFAULT_BASE: usize = 0xc000_0000;

    pub const USER_AREA_END: usize = 0xff000000;
    pub const PAGE_SIZE: usize = 4096;
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Context {}

impl Context {
    pub fn current() -> &'static mut Context {
        unimplemented!();
    }
    pub fn saved() -> &'static mut Context {
        unimplemented!();
    }

    /// Determine whether a process context is valid.
    /// Contexts are valid when the `SATP.VALID` bit is `1`.
    pub fn valid(&self) -> bool {
        unimplemented!();
    }

    /// Invalidate a context by setting its `SATP.VALID` bit to 0.
    pub fn invalidate(&mut self) {
        unimplemented!();
    }

    pub fn get_stack(&self) -> usize {
        unimplemented!();
    }
    pub fn init(&mut self, entrypoint: usize, stack: usize) {}
}

#[cfg(test)]
#[no_mangle]
pub fn _xous_syscall_rust(
    nr: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    ret: &mut xous::Result,
) {
    unimplemented!();
}

#[cfg(test)]
#[no_mangle]
fn _xous_syscall(
    nr: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
    ret: &mut xous::Result,
) {
    unimplemented!();
}

pub fn virt_to_phys(virt: usize) -> Result<usize, xous::Result> {
    unimplemented!();
}

pub fn address_available(virt: usize) -> bool {
    virt_to_phys(virt).is_err()
}
