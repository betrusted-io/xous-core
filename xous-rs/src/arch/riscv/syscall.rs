extern "C" {
    fn _xous_syscall(
        nr: usize,
        a1: usize,
        a2: usize,
        a3: usize,
        a4: usize,
        a5: usize,
        a6: usize,
        a7: usize,
        ret: *mut crate::Result,
    );
    fn _xous_syscall_ptr(
        args: &[usize; 8],
        ret: *mut crate::Result,
    );
}

use crate::definitions::SysCallResult;
use crate::syscall::SysCall;

pub fn syscall(call: SysCall) -> SysCallResult {
    let mut ret = crate::Result::Ok;
    let args = call.as_args();
    unsafe {
        _xous_syscall_ptr(
            &args, &mut ret as *mut _,
        )
    };
    match ret {
        crate::Result::Error(e) => Err(e),
        other => Ok(other),
    }
}

// pub fn syscall(call: SysCall) -> SysCallResult {
//     let mut ret = crate::Result::Ok;
//     let args = call.as_args();
//     unsafe {
//         _xous_syscall(
//             args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], &mut ret as *mut _,
//         )
//     };
//     match ret {
//         crate::Result::Error(e) => Err(e),
//         other => Ok(other),
//     }
// }
