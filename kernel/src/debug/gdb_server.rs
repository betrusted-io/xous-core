use gdbstub::common::Tid;
use gdbstub::target::ext::base::multithread::{
    GdbInterrupt, MultiThreadOps, ResumeAction, ThreadStopReason,
};
use gdbstub::{DisconnectReason, GdbStubError};

use gdbstub::state_machine::GdbStubStateMachine;
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::{Target, TargetResult};

pub struct XousTarget {
    pid: Option<xous_kernel::PID>,
}
pub struct XousDebugState<'a> {
    pub target: XousTarget,
    pub server: GdbStubStateMachine<'a, XousTarget, super::Uart>,
}

pub static mut GDB_STATE: Option<XousDebugState> = None;
pub static mut GDB_BUFFER: [u8; 4096] = [0u8; 4096];

impl XousTarget {
    pub fn new() -> XousTarget {
        XousTarget {
            // pid: Some(crate::services::SystemServices::with_mut(
            //     |system_services| system_services.current_pid(),
            pid: xous_kernel::PID::new(2),
        }
    }
    pub fn pid(&self) -> &Option<xous_kernel::PID> {
        &self.pid
    }
}

impl Target for XousTarget {
    type Arch = gdbstub_arch::riscv::Riscv32;
    type Error = &'static str;
    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        BaseOps::MultiThread(self)
    }
    fn breakpoints(&mut self) -> Option<gdbstub::target::ext::breakpoints::BreakpointsOps<Self>> {
        Some(self)
    }
}

impl MultiThreadOps for XousTarget {
    fn resume(
        &mut self,
        default_resume_action: ResumeAction,
        _gdb_interrupt: GdbInterrupt<'_>,
    ) -> Result<Option<ThreadStopReason<u32>>, Self::Error> {
        unsafe { HALTED = false };
        match default_resume_action {
            ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
                Err("single-stepping not supported")?
            }
            _ => (),
        }

        crate::services::SystemServices::with_mut(|system_services| {
            system_services.continue_process(self.pid.unwrap()).unwrap()
        });
        Ok(None)
    }

    fn clear_resume_actions(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_resume_action(&mut self, _tid: Tid, action: ResumeAction) -> Result<(), Self::Error> {
        match action {
            ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
                Err("single-stepping resume action not supported")
            }
            ResumeAction::Continue | ResumeAction::ContinueWithSignal(_) => Ok(()),
        }
    }

    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::riscv::reg::RiscvCoreRegs<u32>,
        tid: Tid,
    ) -> TargetResult<(), Self> {
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();
            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = self.pid.unwrap();
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            let process = crate::arch::process::Process::current();
            let thread = process.thread(tid.get());
            regs.x[0] = 0;
            for (dbg_reg, thr_reg) in regs.x[1..].iter_mut().zip(thread.registers.iter()) {
                *dbg_reg = (*thr_reg) as u32;
            }
            regs.pc = (thread.sepc) as u32;

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
        });
        Ok(())
    }

    fn write_registers(
        &mut self,
        regs: &gdbstub_arch::riscv::reg::RiscvCoreRegs<u32>,
        tid: Tid,
    ) -> TargetResult<(), Self> {
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = self.pid.unwrap();
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            let mut process = crate::arch::process::Process::current();
            let thread = process.thread_mut(tid.get());
            for (thr_reg, dbg_reg) in thread.registers.iter_mut().zip(regs.x[1..].iter()) {
                *thr_reg = (*dbg_reg) as usize;
            }
            thread.sepc = (regs.pc) as usize;

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
        });
        Ok(())
    }

    fn read_addrs(
        &mut self,
        start_addr: u32,
        data: &mut [u8],
        _tid: Tid, // same address space for each core
    ) -> TargetResult<(), Self> {
        let mut current_addr = start_addr;
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = self.pid.unwrap();
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            data.iter_mut().for_each(|b| {
                *b = crate::arch::mem::peek_memory(current_addr as *mut u8).unwrap_or(0xff);
                current_addr += 1;
            });

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
        });
        Ok(())
    }

    fn write_addrs(
        &mut self,
        start_addr: u32,
        data: &[u8],
        _tid: Tid, // same address space for each core
    ) -> TargetResult<(), Self> {
        let mut current_addr = start_addr;
        // gprintln!("Writing data to {:08x}", start_addr);
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = self.pid.unwrap();
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            data.iter().for_each(|b| {
                if let Err(_e) = crate::arch::mem::poke_memory(current_addr as *mut u8, *b) {
                    panic!("couldn't poke memory: {:?}", _e);
                    // gprintln!("Error writing to {:08x}: {:?}", current_addr, e);
                }
                current_addr += 1;
            });

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
        });
        Ok(())
    }

    fn list_active_threads(
        &mut self,
        register_thread: &mut dyn FnMut(Tid),
    ) -> Result<(), Self::Error> {
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            let debugging_pid = self.pid.unwrap();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            crate::arch::process::Process::current().for_each_thread_mut(|tid, _thr| {
                println!("Registering thread {}", tid);
                register_thread(Tid::new(tid).unwrap());
            });

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
        });
        Ok(())
    }
}

impl gdbstub::target::ext::breakpoints::Breakpoints for XousTarget {
    fn hw_breakpoint(
        &mut self,
    ) -> Option<gdbstub::target::ext::breakpoints::HwBreakpointOps<Self>> {
        Some(self)
    }
}

impl gdbstub::target::ext::breakpoints::HwBreakpoint for XousTarget {
    #[inline(never)]
    fn add_hw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
        Ok(false)
    }

    #[inline(never)]
    fn remove_hw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
        Ok(false)
    }
}

pub fn handle(b: u8) -> bool {
    if let Some(XousDebugState {
        mut target,
        server: gdb,
    }) = unsafe { GDB_STATE.take() }
    {
        // gprintln!("Adding char: {}", b as char);
        let new_gdb = match gdb {
            GdbStubStateMachine::Pump(gdb_state) => match gdb_state.pump(&mut target, b) {
                // Remote disconnected -- leave the `GDB_SERVER` as `None`.
                Ok((gdb, Some(DisconnectReason::TargetTerminated(_r)))) => {
                    // gprintln!("Target halted: {}", r);
                    crate::services::SystemServices::with_mut(|system_services| {
                        system_services
                            .suspend_process(target.pid().unwrap())
                            .unwrap()
                    });
                    gdb
                }
                Ok((_, Some(_disconnect_reason))) => {
                    cleanup();
                    match _disconnect_reason {
                        DisconnectReason::Disconnect => println!("GDB Disconnected"),
                        DisconnectReason::TargetExited(_) => println!("Target exited"),
                        DisconnectReason::TargetTerminated(_) => unreachable!(),
                        DisconnectReason::Kill => println!("GDB sent a kill command"),
                    }
                    return true;
                }
                Err(GdbStubError::TargetError(e)) => {
                    cleanup();
                    println!("Target raised a fatal error: {}", e);
                    return true;
                }
                Err(e) => {
                    cleanup();
                    println!("gdbstub internal error: {}", e);
                    return true;
                }
                Ok((gdb, None)) => gdb,
            },

            GdbStubStateMachine::DeferredStopReason(gdb_state) => {
                match gdb_state.deferred_stop_reason(&mut target, ThreadStopReason::DoneStep) {
                    Ok((gdb, None)) => {
                        crate::services::SystemServices::with_mut(|system_services| {
                            system_services
                                .suspend_process(target.pid().unwrap())
                                .unwrap()
                        });
                        gdb
                    }
                    Ok((_, Some(disconnect_reason))) => {
                        cleanup();
                        println!("client disconnected: {:?}", disconnect_reason);
                        return true;
                    }
                    Err(e) => {
                        cleanup();
                        println!("deferred_stop_reason_error: {:?}", e);
                        return true;
                    }
                }
            }
        };
        unsafe {
            GDB_STATE = Some(XousDebugState {
                target,
                server: new_gdb,
            })
        };
        true
    } else {
        false
    }
}

pub fn setup() {
    match gdbstub::GdbStubBuilder::new(super::Uart {})
        .with_packet_buffer(unsafe { &mut GDB_BUFFER })
        .build()
    {
        Ok(gdb) => match gdb.run_state_machine() {
            Ok(state) => unsafe {
                GDB_STATE = Some(XousDebugState {
                    target: XousTarget::new(),
                    server: state,
                });
                super::DEBUG_OUTPUT = Some(&mut GUART);
                HALTED = true;
            },
            Err(e) => println!("Unable to start GDB state machine: {}", e),
        },
        Err(e) => println!("Unable to start GDB server: {}", e),
    }
}

fn cleanup() {
    unsafe { super::DEBUG_OUTPUT = Some(&mut super::UART) };
}

// Support printf() when running under a debugger
pub struct GUart {}
static mut GUART: GUart = GUart {};
static mut HALTED: bool = false;

impl core::fmt::Write for GUart {
    fn write_str(&mut self, s: &str) -> Result<(), core::fmt::Error> {
        fn putc(checksum: u8, c: u8) -> u8 {
            unsafe { super::UART.putc(c) };
            checksum.wrapping_add(c)
        }
        fn puth(checksum: u8, c: u8) -> u8 {
            let hex = b"0123456789abcdef";
            let c = c as usize;
            let checksum = putc(checksum, hex[(c >> 4) & 0xf]);
            putc(checksum, hex[(c >> 0) & 0xf])
        }

        // Can't write when we're halted
        if unsafe { HALTED } {
            return Ok(());
        }

        let mut checksum = 0u8;
        putc(0, b'$');

        checksum = putc(checksum, b'O');
        for c in s.as_bytes() {
            checksum = puth(checksum, *c);
        }

        putc(0, b'#');
        puth(0, checksum);

        Ok(())
    }
}
