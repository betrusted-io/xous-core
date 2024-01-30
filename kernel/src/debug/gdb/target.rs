use gdbstub::arch::SingleStepGdbBehavior;
use gdbstub::target::ext::base::BaseOps;
use gdbstub::target::ext::breakpoints::BreakpointsOps;
use gdbstub::target::ext::extended_mode::ExtendedModeOps;
use gdbstub::target::ext::monitor_cmd::MonitorCmdOps;
use gdbstub::target::Target;

use super::XousTarget;

impl Target for XousTarget {
    type Arch = gdbstub_arch::riscv::Riscv32;
    type Error = &'static str;

    fn base_ops(&mut self) -> BaseOps<Self::Arch, Self::Error> { BaseOps::MultiThread(self) }

    fn support_breakpoints(&mut self) -> Option<BreakpointsOps<Self>> { Some(self) }

    /// Opt in to having GDB handle breakpoints for us. This allows for an unlimited number
    /// of breakpoints without having us keep track of the breakpoints ourselves, but
    /// doesn't work with XIP programs.
    fn guard_rail_implicit_sw_breakpoints(&self) -> bool { true }

    fn guard_rail_single_step_gdb_behavior(&self) -> SingleStepGdbBehavior { SingleStepGdbBehavior::Required }

    fn support_monitor_cmd(&mut self) -> Option<MonitorCmdOps<'_, Self>> { Some(self) }

    fn support_extended_mode(&mut self) -> Option<ExtendedModeOps<'_, Self>> { Some(self) }
}
