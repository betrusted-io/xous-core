use gdbstub::target::TargetResult;
use gdbstub::target::ext::breakpoints::{Breakpoints, SwBreakpoint, SwBreakpointOps};

use super::XousTarget;

impl Breakpoints for XousTarget {
    /// Indicate that we do not support software breakpoints. If we ever decide
    /// to add breakpoint support, we will need to implement this ourselves. However,
    /// by reporting `None` here we allow GDB to manage breakpoints for us.
    fn support_sw_breakpoint(&mut self) -> Option<SwBreakpointOps<'_, Self>> {
        // Some(self)
        None
    }
}

impl SwBreakpoint for XousTarget {
    fn add_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> { Ok(false) }

    fn remove_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> { Ok(false) }
}
