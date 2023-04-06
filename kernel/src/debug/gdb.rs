use gdbstub::common::{Signal, Tid};
use gdbstub::stub::state_machine::GdbStubStateMachine;
use gdbstub::stub::{GdbStubBuilder, GdbStubError, MultiThreadStopReason};
use gdbstub::target;
use gdbstub::target::ext::base::multithread::{MultiThreadBase, MultiThreadResume};
use gdbstub::target::ext::base::single_register_access::{
    SingleRegisterAccess, SingleRegisterAccessOps,
};
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::ext::monitor_cmd::MonitorCmd;
use gdbstub::target::TargetError;
use gdbstub::target::{Target, TargetResult};
use gdbstub_arch::riscv::reg::id::RiscvRegId;

use crate::io::SerialRead;
use crate::platform::precursor::gdbuart::GdbUart;

use core::convert::TryInto;

pub struct XousTarget {
    pid: Option<xous_kernel::PID>,
}
pub struct XousDebugState<'a> {
    pub target: XousTarget,
    pub server: GdbStubStateMachine<'a, XousTarget, crate::platform::precursor::gdbuart::GdbUart>,
}

pub static mut GDB_STATE: Option<XousDebugState> = None;
pub static mut GDB_BUFFER: [u8; 4096] = [0u8; 4096];

fn receive_irq(uart: &mut GdbUart) {
    while let Some(c) = uart.getc() {
        advance_gdb_or_recreate(Some(c));
    }
}

impl XousTarget {
    pub fn new() -> XousTarget {
        XousTarget { pid: None }
    }
}

impl Target for XousTarget {
    type Arch = gdbstub_arch::riscv::Riscv32;
    type Error = &'static str;

    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> {
        BaseOps::MultiThread(self)
    }

    fn support_breakpoints(
        &mut self,
    ) -> Option<gdbstub::target::ext::breakpoints::BreakpointsOps<Self>> {
        Some(self)
    }

    fn support_monitor_cmd(&mut self) -> Option<target::ext::monitor_cmd::MonitorCmdOps<'_, Self>> {
        Some(self)
    }
}

impl MultiThreadBase for XousTarget {
    fn read_registers(
        &mut self,
        regs: &mut gdbstub_arch::riscv::reg::RiscvCoreRegs<u32>,
        tid: Tid,
    ) -> TargetResult<(), Self> {
        let Some(pid) = self.pid else {
            for entry in regs.x.iter_mut() {
                *entry = 0;
            }
            return Ok(());
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();
            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
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
        let Some(pid) = self.pid else {
            return Ok(())
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
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
        let Some(pid) = self.pid else {
            for entry in data.iter_mut() { *entry = 0 };
            return Ok(());
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
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
        let Some(pid) = self.pid else {
            return Ok(());
        };
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
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
        let Some(pid) = self.pid else {
            // Register a fake thread
            register_thread(Tid::new(1).unwrap());
            return Ok(());
        };
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            let debugging_pid = pid;

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            crate::arch::process::Process::current().for_each_thread_mut(|tid, _thr| {
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

    fn support_single_register_access(&mut self) -> Option<SingleRegisterAccessOps<'_, Tid, Self>> {
        Some(self)
    }

    fn support_resume(
        &mut self,
    ) -> Option<target::ext::base::multithread::MultiThreadResumeOps<'_, Self>> {
        Some(self)
    }
}

impl SingleRegisterAccess<Tid> for XousTarget {
    fn read_register(
        &mut self,
        tid: Tid,
        reg_id: RiscvRegId<u32>,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        // For the case of no PID, fake a value
        let Some(pid) = self.pid else {
            buf.copy_from_slice(&0u32.to_le_bytes());
            return Ok(buf.len());
        };

        let reg_id = match reg_id {
            RiscvRegId::Gpr(0) => {
                buf.copy_from_slice(&0u32.to_le_bytes());
                return Ok(buf.len());
            }
            RiscvRegId::Gpr(x) => x as usize - 1,
            RiscvRegId::Pc => 32,
            _ => {
                return Err(TargetError::Fatal("register out of range"));
            }
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();
            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            let process = crate::arch::process::Process::current();
            let thread = process.thread(tid.get());
            let reg = if reg_id == 32 {
                &thread.sepc
            } else {
                match thread.registers.get(reg_id) {
                    Some(val) => val,
                    None => return Err(TargetError::Fatal("register out of range")),
                }
            };

            buf.copy_from_slice(&reg.to_le_bytes());

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
            Ok(buf.len())
        })
    }

    fn write_register(
        &mut self,
        tid: Tid,
        reg_id: RiscvRegId<u32>,
        val: &[u8],
    ) -> TargetResult<(), Self> {
        let Some(pid) = self.pid else {
            return Ok(())
        };

        let w = u32::from_le_bytes(
            val.try_into()
                .map_err(|_| TargetError::Fatal("invalid data"))?,
        );

        let reg_id = match reg_id {
            RiscvRegId::Gpr(0) => {
                return Ok(());
            }
            RiscvRegId::Gpr(x) => x as usize - 1,
            RiscvRegId::Pc => 32,
            _ => {
                return Err(TargetError::Fatal("register out of range"));
            }
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();
            let mut process = crate::arch::process::Process::current();
            let thread = process.thread_mut(tid.get());
            let reg = if reg_id == 32 {
                &mut thread.sepc
            } else {
                match thread.registers.get_mut(reg_id) {
                    Some(val) => val,
                    None => return Err(TargetError::Fatal("register out of range")),
                }
            };
            *reg = w as usize;

            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
            Ok(())
        })
    }
}

impl MultiThreadResume for XousTarget {
    fn resume(&mut self) -> Result<(), Self::Error> {
        println!("Resuming process {:?}", self.pid);
        // unsafe { HALTED = false };
        // match default_resume_action {
        //     ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
        //         return Err("single-stepping not supported")
        //     }
        //     _ => (),
        // }

        if let Some(pid) = self.pid {
            crate::services::SystemServices::with_mut(|system_services| {
                system_services.resume_process_from_debug(pid).unwrap()
            });
        }
        Ok(())
    }

    fn clear_resume_actions(&mut self) -> Result<(), Self::Error> {
        println!("Clearing resume actions");
        Ok(())
    }

    fn set_resume_action_continue(
        &mut self,
        tid: Tid,
        signal: Option<Signal>,
    ) -> Result<(), Self::Error> {
        println!(
            "Setting resume action continue on process {:?} thread {:?} with signal: {:?}",
            self.pid, tid, signal
        );
        // match action {
        //     ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
        //         Err("single-stepping resume action not supported")
        //     }
        //     ResumeAction::Continue | ResumeAction::ContinueWithSignal(_) => Ok(()),
        // }
        Ok(())
    }
}

impl target::ext::breakpoints::Breakpoints for XousTarget {
    #[inline(always)]
    fn support_sw_breakpoint(
        &mut self,
    ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
        Some(self)
    }
}

impl target::ext::breakpoints::SwBreakpoint for XousTarget {
    #[inline(never)]
    fn add_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
        println!(
            "GDB asked us to add a software breakpoint at {:08x} of kind {:?}",
            _addr, _kind
        );
        Ok(true)
    }

    #[inline(never)]
    fn remove_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
        println!(
            "GDB asked us to remove a software breakpoint at {:08x} of kind {:?}",
            _addr, _kind
        );
        Ok(true)
    }
}

impl MonitorCmd for XousTarget {
    fn handle_monitor_cmd(
        &mut self,
        cmd: &[u8],
        mut out: target::ext::monitor_cmd::ConsoleOutput<'_>,
    ) -> Result<(), Self::Error> {
        let Ok(cmd) = core::str::from_utf8(cmd) else {
            gdbstub::outputln!(out, "command must be valid UTF-8");
            return Ok(());
        };

        if cmd.starts_with("pr") {
            if let Some(pid_str) = cmd.split_ascii_whitespace().nth(1) {
                // Parse the new PID. If it isn't a valid string, then this will be None
                let new_pid =
                    xous_kernel::PID::new(u8::from_str_radix(pid_str, 10).unwrap_or_default());
                if let Some(previous_pid) = self.pid {
                    crate::services::SystemServices::with_mut(|system_services| {
                        system_services
                            .resume_process_from_debug(previous_pid)
                            .unwrap()
                    });
                }
                // Disallow debugging the kernel. Sad times.
                if new_pid.map(|v| v.get() == 1).unwrap_or(false) {
                    gdbstub::outputln!(out, "Kernel cannot debug itself");
                    self.pid = None;
                    return Ok(());
                }

                self.pid = new_pid;
                if let Some(new_pid) = self.pid {
                    gdbstub::outputln!(out, "Now debugging PID {}", new_pid);
                    crate::services::SystemServices::with_mut(|system_services| {
                        system_services.pause_process_for_debug(new_pid).unwrap()
                    });
                } else {
                    gdbstub::outputln!(out, "No process is selected for debugging");
                }
                return Ok(());
            }
            gdbstub::outputln!(out, "Available processes:");

            crate::services::SystemServices::with(|system_services| {
                for process in &system_services.processes {
                    if !process.free() {
                        gdbstub::outputln!(
                            out,
                            "  {:2} {} {}",
                            process.pid,
                            if self.pid.map(|p| p == process.pid).unwrap_or(false) {
                                '*'
                            } else {
                                ' '
                            },
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                    }
                }
            });
        } else if cmd.starts_with("h") {
            gdbstub::outputln!(out, "Here is a list of help commands:");
            gdbstub::outputln!(out, "  process       Print a list of processes");
            gdbstub::outputln!(out, "  process [n]   Switch to debugging process [n]");
        } else {
            gdbstub::outputln!(out, "command not found -- try 'mon help'");
        }
        Ok(())
    }
}

fn advance_gdb_or_recreate(byte: Option<u8>) {
    advance_gdb_state(byte);
    if unsafe { GDB_STATE.is_none() } {
        println!("GDB went away -- recreating it");
        init();
    }
}

fn advance_gdb_state(byte: Option<u8>) -> bool {
    let Some(XousDebugState {
        mut target,
        server: gdb,
    }) = (unsafe { GDB_STATE.take() }) else {
        println!("No GDB!");
        return false;
    };

    let mut new_gdb = match gdb {
        GdbStubStateMachine::Idle(gdb_stm_inner) => match byte {
            Some(byte) => match gdb_stm_inner.incoming_data(&mut target, byte) {
                Ok(gdb) => gdb,
                Err(e) => {
                    println!("gdbstub error during idle operation: {:?}", e);
                    return true;
                }
            },
            None => {
                println!("gdbstub was idle with no characters?!");
                GdbStubStateMachine::Idle(gdb_stm_inner)
            }
        },

        GdbStubStateMachine::Running(gdb_stm_inner) => {
            // If we're here we were running but have stopped now (either
            // because we hit Ctrl+c in gdb and hence got a serial interrupt
            // or we hit a breakpoint).
            // let conn = gdb_stm_inner.borrow_conn();
            // conn.disable_irq();
            // let data_to_read = conn.peek().unwrap().is_some();
            // let mut stop_reason = target.determine_stop_reason(reason);

            if let Some(byte) = byte {
                match gdb_stm_inner.incoming_data(&mut target, byte) {
                    Ok(pumped_stm) => {
                        if let GdbStubStateMachine::CtrlCInterrupt(_) = pumped_stm {
                            println!(
                                "Looks like we maybe just transitioned from running to ctrl-c?"
                            );
                        } else if let GdbStubStateMachine::Idle(_) = pumped_stm {
                            println!("Looks like we maybe just transitioned from running to idle?");
                        }
                        pumped_stm
                    }
                    Err(GdbStubError::TargetError(e)) => {
                        println!("Target raised a fatal error: {:?}", e);
                        return false;
                    }
                    Err(e) => {
                        println!("gdbstub error in DeferredStopReason.pump: {:?}", e);
                        return false;
                    }
                }
            // } else if let Some(reason) = stop_reason.take() {
            //     match gdb_stm_inner.report_stop(&mut target, MultiThreadStopReason::DoneStep) {
            //         Ok(gdb_stm_new) => gdb_stm_new,
            //         Err(GdbStubError::TargetError(e)) => {
            //             println!("Target raised a fatal error {:?}", e);
            //             return false;
            //         }
            //         Err(e) => {
            //             println!("gdbstub internal error {:?}", e);
            //             return false;
            //         }
            //     }
            // } else if target.resume_with.is_some() {
            //     // // We don't have a `stop_reason` and we don't have something
            //     // // to read on the line. This probably means we're done and
            //     // // we should run again.
            //     // conn.enable_irq();
            //     // let r = GDB_STUB
            //     //     .lock()
            //     //     .replace(GdbStubStateMachine::Running(gdb_stm_inner));
            //     // assert!(
            //     //     r.is_none(),
            //     //     "Put something in GDB_STUB which we shouldn't have..."
            //     // );
            //     // return false;
            //     GdbStubStateMachine::Running(gdb_stm_inner)
            } else {
                panic!("Can't happen?");
            }
        }

        GdbStubStateMachine::CtrlCInterrupt(_gdb_stm_inner) => {
            panic!("debug server was in an unexpected state");
        }

        GdbStubStateMachine::Disconnected(_gdb_stm_inner) => {
            println!("GdbStubStateMachine::Disconnected byebye");
            return false;
        }
    };

    if let GdbStubStateMachine::CtrlCInterrupt(gdb_stm_inner) = new_gdb {
        println!("Just transitioned into CtrlCInterrupt");
        match gdb_stm_inner.interrupt_handled(&mut target, Some(MultiThreadStopReason::DoneStep)) {
            Ok(gdb) => new_gdb = gdb,
            Err(e) => {
                println!("gdbstub error during ctrl-c interrupt{:?}", e);
                return false;
            }
        }

        if let Some(pid) = target.pid {
            println!("Starting debug on process {:?}", pid);
            crate::services::SystemServices::with_mut(|system_services| {
                system_services.pause_process_for_debug(pid).unwrap()
            });
        } else {
            println!("No process specified! Not debugging");
        }
    }

    unsafe {
        GDB_STATE = Some(XousDebugState {
            target,
            server: new_gdb,
        })
    };
    true
}

pub fn init() {
    let uart = GdbUart::new(receive_irq).unwrap();
    let mut target = XousTarget::new();

    match GdbStubBuilder::new(uart)
        .with_packet_buffer(unsafe { &mut GDB_BUFFER })
        .build()
    {
        Ok(gdb) => match gdb.run_state_machine(&mut target) {
            Ok(state) => unsafe {
                GDB_STATE = Some(XousDebugState {
                    target: target,
                    server: state,
                });
                HALTED = true;
            },
            Err(e) => println!("Unable to start GDB state machine: {}", e),
        },
        Err(e) => println!("Unable to start GDB server: {}", e),
    }
}

static mut HALTED: bool = false;
