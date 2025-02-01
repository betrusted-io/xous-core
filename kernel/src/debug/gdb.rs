use gdbstub::common::{Signal, Tid};
use gdbstub::conn::Connection;
use gdbstub::stub::state_machine::GdbStubStateMachine;
use gdbstub::stub::{GdbStubBuilder, GdbStubError, MultiThreadStopReason};
use gdbstub::target::Target;

use crate::io::SerialRead;
use crate::platform::precursor::gdbuart::GdbUart;

mod breakpoints;
mod current_active_pid;
mod extended_mode;
mod monitor;
mod multi_thread_base;
mod multi_thread_resume;
mod multi_thread_single_step;
mod single_register_access;
mod target;

#[cfg(target_arch = "riscv32")]
#[path = "gdb/riscv.rs"]
mod cpu;

pub struct XousTarget {
    pid: Option<xous_kernel::PID>,

    inner: cpu::XousTargetInner,
}

pub struct XousDebugState<'a> {
    pub target: XousTarget,
    pub server: GdbStubStateMachine<'a, XousTarget, crate::platform::precursor::gdbuart::GdbUart>,
}

static mut GDB_STATE: Option<XousDebugState> = None;
static mut GDB_BUFFER: [u8; 4096] = [0u8; 4096];

trait ProcessPid {
    fn pid(&self) -> Option<xous_kernel::PID>;
    fn take_pid(&mut self) -> Option<xous_kernel::PID>;
}

impl ProcessPid for XousTarget {
    fn pid(&self) -> Option<xous_kernel::PID> { self.pid }

    fn take_pid(&mut self) -> Option<xous_kernel::PID> { self.pid.take() }
}

struct MicroRingBuf<const N: usize> {
    buffer: [u8; N],
    head: usize,
    tail: usize,
}

impl<const N: usize> Default for MicroRingBuf<N> {
    fn default() -> Self { MicroRingBuf { buffer: [0u8; N], head: 0, tail: 0 } }
}

impl<const N: usize> MicroRingBuf<N> {
    // pub fn capacity(&self) -> usize {
    //     self.buffer.len()
    // }
    // pub fn len(&self) -> usize {
    //     self.head.wrapping_sub(self.tail) % N
    // }

    pub fn is_full(&self) -> bool { (self.tail.wrapping_sub(1) % N) == self.head }

    pub fn try_push(&mut self, val: u8) -> Result<(), ()> {
        if self.is_full() {
            return Err(());
        }
        self.buffer[self.head] = val;
        self.head = (self.head + 1) % N;
        Ok(())
    }

    pub fn try_pop(&mut self) -> Option<u8> {
        if self.tail == self.head {
            return None;
        }
        let val = self.buffer[self.tail];
        self.tail = (self.tail + 1) % N;
        Some(val)
    }
}

fn receive_irq(uart: &mut GdbUart) {
    let mut buffer = MicroRingBuf::<32>::default();
    loop {
        // Try to fill up the ring buffer with as many characters
        // as can fit. This is to compensate for the fact that we do
        // all of this processing in an interrupt context, and the
        // hardware UART buffer is only a few characters deep.
        while !buffer.is_full() {
            if let Some(c) = uart.getc() {
                buffer.try_push(c).ok();
            } else {
                break;
            }
        }

        // If there is a character in the buffer, process it. Otherwise,
        // we're done.
        let Some(c) = buffer.try_pop() else { break };
        process_character(c);

        // If the GDB server goes away for some reason, reconstitute it
        unsafe {
            if GDB_STATE.is_none() {
                init();
            }
        }
    }
}

impl XousTarget {
    pub fn new() -> XousTarget { XousTarget { pid: None, inner: cpu::XousTargetInner::default() } }
}

fn state_can_accept_characters<'a, T: Target + ProcessPid, C: Connection>(
    machine: &GdbStubStateMachine<'a, T, C>,
) -> bool {
    match machine {
        GdbStubStateMachine::Idle(_) | GdbStubStateMachine::Running(_) => true,
        GdbStubStateMachine::CtrlCInterrupt(_) | GdbStubStateMachine::Disconnected(_) => false,
    }
}

fn ensure_can_accept_characters_inner<'a, T: Target + ProcessPid, C: Connection>(
    machine: GdbStubStateMachine<'a, T, C>,
    target: &mut T,
    recurse_count: usize,
) -> Option<GdbStubStateMachine<'a, T, C>> {
    if recurse_count == 0 {
        return None;
    }

    match machine {
        GdbStubStateMachine::Idle(_) | GdbStubStateMachine::Running(_) => Some(machine),
        GdbStubStateMachine::CtrlCInterrupt(gdb_stm_inner) => {
            if let Some(pid) = target.pid() {
                crate::services::SystemServices::with_mut(|system_services| {
                    if let Err(e) = system_services.pause_process_for_debug(pid) {
                        println!("Unable to pause process {:?} for debug: {:?}", pid, e);
                    }
                });
            }

            let Ok(new_server) =
                gdb_stm_inner.interrupt_handled(target, Some(MultiThreadStopReason::Signal(Signal::SIGINT)))
            else {
                return None;
            };
            ensure_can_accept_characters_inner(new_server, target, recurse_count - 1)
        }
        GdbStubStateMachine::Disconnected(gdb_stm_inner) => {
            if let Some(pid) = target.take_pid() {
                crate::services::SystemServices::with_mut(|system_services| {
                    system_services.resume_process_from_debug(pid).unwrap()
                });
            }

            ensure_can_accept_characters_inner(gdb_stm_inner.return_to_idle(), target, recurse_count - 1)
        }
    }
}

fn ensure_can_accept_characters<'a, T: Target + ProcessPid, C: Connection>(
    machine: GdbStubStateMachine<'a, T, C>,
    target: &mut T,
) -> Option<GdbStubStateMachine<'a, T, C>> {
    ensure_can_accept_characters_inner(machine, target, 4)
}

/// Advance the GDB state.
///
/// Two states accept characters:
///
///     GdbStubStateMachine::Idle
///     GdbStubStateMachine::Running
///
/// Two states exist merely to transition to other states:
///
///     GdbStubStateMachine::CtrlCInterrupt
///     GdbStubStateMachine::Disconnected
fn process_character(byte: u8) {
    let XousDebugState { mut target, server } = unsafe {
        GDB_STATE.take().unwrap_or_else(|| {
            init();
            GDB_STATE.take().unwrap()
        })
    };

    if !state_can_accept_characters(&server) {
        println!("GDB server was not in a state to accept characters");
        return;
    }

    let new_server = match server {
        GdbStubStateMachine::Idle(gdb_stm_inner) => {
            let Ok(gdb) = gdb_stm_inner
                .incoming_data(&mut target, byte)
                .map_err(|e| println!("gdbstub error during idle operation: {:?}", e))
            else {
                return;
            };
            gdb
        }

        GdbStubStateMachine::Running(gdb_stm_inner) => {
            // If we're here we were running but have stopped now (either
            // because we hit Ctrl+c in gdb and hence got a serial interrupt
            // or we hit a breakpoint).

            match gdb_stm_inner.incoming_data(&mut target, byte) {
                Ok(pumped_stm) => pumped_stm,
                Err(GdbStubError::TargetError(e)) => {
                    println!("Target raised a fatal error: {:?}", e);
                    return;
                }
                Err(e) => {
                    println!("gdbstub error in DeferredStopReason.pump: {:?}", e);
                    return;
                }
            }
        }

        _ => {
            println!("GDB is in an unexpected state!");
            return;
        }
    };

    // If the user just hit Ctrl-C, then remove the pending interrupt that may or may not exist.
    if let GdbStubStateMachine::CtrlCInterrupt(_) = &new_server {
        target.unpatch_stepi(Tid::new(1).unwrap()).ok();
    }

    let Some(server) = ensure_can_accept_characters(new_server, &mut target) else {
        println!("Couldn't convert GDB into a state that accepts characters");
        return;
    };

    unsafe { GDB_STATE = Some(XousDebugState { target, server }) };
}

pub fn report_stop(_pid: xous_kernel::PID, tid: xous_kernel::TID, _pc: usize) {
    let Some(XousDebugState { mut target, server: gdb }) = (unsafe { GDB_STATE.take() }) else {
        println!("No GDB!");
        return;
    };

    target.unpatch_stepi(Tid::new(tid).unwrap()).ok();

    let GdbStubStateMachine::Running(inner) = gdb else {
        println!("GDB state machine was in an invalid state");
        return;
    };

    let Ok(new_gdb) = inner.report_stop(
        &mut target,
        MultiThreadStopReason::SignalWithThread {
            signal: Signal::EXC_BREAKPOINT,
            tid: Tid::new(tid).unwrap(),
        },
    ) else {
        println!("Unable to report stop");
        return;
    };

    unsafe { GDB_STATE = Some(XousDebugState { target, server: new_gdb }) };
}

pub fn report_terminated(pid: xous_kernel::PID) {
    let Some(XousDebugState { mut target, server: gdb }) = (unsafe { GDB_STATE.take() }) else {
        println!("No GDB!");
        return;
    };

    let new_gdb = match gdb {
        GdbStubStateMachine::Running(inner) => {
            match inner.report_stop(&mut target, MultiThreadStopReason::Signal(Signal::EXC_BAD_ACCESS)) {
                Ok(new_gdb) => new_gdb,
                Err(e) => {
                    println!("Unable to report stop: {:?}", e);
                    return;
                }
            }
        }
        GdbStubStateMachine::CtrlCInterrupt(_inner) => {
            println!("GDB state was in CtrlCInterrupt, which shouldn't be possible!");
            return;
        }
        GdbStubStateMachine::Disconnected(_inner) => {
            println!("GDB state was in Disconnect, which shouldn't be possible!");
            return;
        }
        GdbStubStateMachine::Idle(inner) => {
            println!("Please connect a debugger to debug process {}", pid);
            GdbStubStateMachine::Idle(inner)
        }
    };

    unsafe { GDB_STATE = Some(XousDebugState { target, server: new_gdb }) };
}

pub fn init() {
    let mut uart = GdbUart::new(receive_irq).unwrap();
    uart.enable();
    let mut target = XousTarget::new();

    let server = GdbStubBuilder::new(uart)
        .with_packet_buffer(unsafe { &mut GDB_BUFFER })
        .build()
        .expect("unable to build gdb server")
        .run_state_machine(&mut target)
        .expect("unable to start gdb state machine");
    unsafe {
        GDB_STATE = Some(XousDebugState { target, server });
    }
}
