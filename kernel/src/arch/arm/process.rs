// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use crate::services::ProcessInner;
use xous_kernel::{ProcessInit, ProcessStartup, ThreadInit, PID, TID};

pub const MAX_THREAD: TID = 31;
pub const EXCEPTION_TID: TID = 1;
pub const INITIAL_TID: TID = 2;
pub const IRQ_TID: TID = 0;

pub const DEFAULT_STACK_SIZE: usize = 128 * 1024;
pub const MAX_PROCESS_COUNT: usize = 64;

/// This is the address a program will jump to in order to return from an ISR.
pub const RETURN_FROM_ISR: usize = 0xff80_2000;

/// This is the address a thread will return to when it exits.
pub const EXIT_THREAD: usize = 0xff80_3000;

/// This is the address a thread will return to when it finishes handling an exception.
pub const RETURN_FROM_EXCEPTION_HANDLER: usize = 0xff80_4000;

pub fn current_pid() -> PID {
    todo!();
}
pub fn current_tid() -> TID {
    todo!();
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct Process {
    dummy: usize,
}

impl Process {
    pub fn current() -> Process {
        todo!()
    }

    pub fn activate(&mut self) -> Result<(), xous_kernel::Error> {
        todo!()
    }

    pub fn with_inner<F, R>(f: F) -> R
    where
        F: FnOnce(&ProcessInner) -> R,
    {
        todo!();
    }

    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&Process) -> R,
    {
        todo!();
    }

    pub fn with_current_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut Process) -> R,
    {
        todo!();
    }
    
    pub fn with_inner_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ProcessInner) -> R,
    {
        todo!();
    }

    pub fn current_thread_mut(&mut self) -> &mut Thread {
        todo!();
    }

    pub fn current_thread(&self) -> &Thread {
        todo!();
    }

    pub fn current_tid(&self) -> TID {
        todo!();
    }

    pub fn thread_exists(&self, _tid: TID) -> bool {
        todo!();
    }

    /// Set the current thread number.
    pub fn set_tid(&mut self, _thread: TID) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    pub fn thread_mut(&mut self, _thread: TID) -> &mut Thread {
        todo!();
    }

    pub fn thread(&self, _thread: TID) -> &Thread {
        todo!();
    }

    #[cfg(feature="gdb-stub")]
    pub fn for_each_thread_mut<F>(&self, mut _op: F)
    where
        F: FnMut(TID, &Thread),
    {
        todo!();
    }

    pub fn find_free_thread(&self) -> Option<TID> {
        todo!();
    }

    pub fn set_thread_result(&mut self, thread_nr: TID, result: xous_kernel::Result) {
        todo!();
    }

    pub fn retry_instruction(&mut self, _tid: TID) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    /// Initialize this process thread with the given entrypoint and stack
    /// addresses.
    pub fn setup_process(_pid: PID, _thread_init: ThreadInit) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    pub fn setup_thread(
        &mut self,
        _new_tid: TID,
        _setup: ThreadInit,
    ) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    /// Destroy a given thread and return its return value.
    ///
    /// # Returns
    ///     The return value of the function
    ///
    /// # Errors
    ///     xous::ThreadNotAvailable - the thread did not exist
    pub fn destroy_thread(&mut self, _tid: TID) -> Result<usize, xous_kernel::Error> {
        todo!();
    }

    pub fn print_all_threads(&self) {
        todo!();
    }

    pub fn print_current_thread(&self) {
        todo!();
    }

    pub fn print_thread(_tid: TID, _thread: &Thread) {
        todo!();
    }

    pub fn create(
        _pid: PID,
        _init_data: ProcessInit,
        _services: &mut crate::SystemServices,
    ) -> Result<ProcessStartup, xous_kernel::Error> {
        todo!();
    }

    pub fn destroy(_pid: PID) -> Result<(), xous_kernel::Error> {
        todo!();
    }

    pub fn find_thread<F>(&self, _op: F) -> Option<(TID, &mut Thread)>
    where
        F: Fn(TID, &Thread) -> bool,
    {
        todo!();
    }
}

#[derive(Copy, Clone, Debug, Default)]
/// Everything required to keep track of a single thread of execution.
pub struct Thread {}

impl Thread {
    /// The current stack pointer for this thread
    pub fn stack_pointer(&self) -> usize {
        todo!();
    }

    pub fn a0(&self) -> usize {
        todo!();
    }

    pub fn a1(&self) -> usize {
        todo!();
    }
}

#[repr(C)]
#[cfg(baremetal)]
pub struct InitialProcess {
    pub satp: usize,
    pub entrypoint: usize,
    pub sp: usize,
}

pub fn set_current_pid(pid: PID) {
}