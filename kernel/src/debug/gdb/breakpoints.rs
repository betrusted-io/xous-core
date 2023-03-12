use gdbstub::target::ext::breakpoints::Breakpoints;

use super::XousTarget;

impl Breakpoints for XousTarget {
    // fn support_sw_breakpoint(
    //     &mut self,
    // ) -> Option<target::ext::breakpoints::SwBreakpointOps<'_, Self>> {
    //     Some(self)
    // }
}

// impl target::ext::breakpoints::SwBreakpoint for XousTarget {
//     fn add_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
//         println!(
//             "GDB asked us to add a software breakpoint at {:08x} of kind {:?}",
//             _addr, _kind
//         );
//         Ok(true)
//     }

//     fn remove_sw_breakpoint(&mut self, _addr: u32, _kind: usize) -> TargetResult<bool, Self> {
//         println!(
//             "GDB asked us to remove a software breakpoint at {:08x} of kind {:?}",
//             _addr, _kind
//         );
//         Ok(true)
//     }
// }
