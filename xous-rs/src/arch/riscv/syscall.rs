extern "Rust" {
    fn _xous_syscall(
        nr: usize,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
        ret: &mut crate::Result,
    );
}

use crate::definitions::SysCallResult;
use crate::syscall::SysCall;

pub fn syscall(call: SysCall) -> SysCallResult {
    let mut args = call.as_args();
    unsafe {
        core::arch::asm!(
            "ecall",
            inlateout("a0") args[0],
            inlateout("a1") args[1],
            inlateout("a2") args[2],
            inlateout("a3") args[3],
            inlateout("a4") args[4],
            inlateout("a5") args[5],
            inlateout("a6") args[6],
            inlateout("a7") args[7],
        )
    };
    match crate::definitions::Result::from_args(args) {
        crate::Result::Error(e) => Err(e),
        other => Ok(other),
    }
}
