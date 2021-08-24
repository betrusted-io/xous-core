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
    let mut ret = crate::Result::Ok;
    let args = call.as_args();
    unsafe {
        _xous_syscall(
            args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], &mut ret,
        )
    };
    match ret {
        crate::Result::Error(e) => Err(e),
        other => Ok(other),
    }
}
