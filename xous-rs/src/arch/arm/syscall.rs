use core::arch::asm;

use crate::definitions::SysCallResult;
use crate::syscall::SysCall;

#[repr(C)]
#[derive(Debug)]
pub struct Arguments {
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
}

impl Arguments {
    fn new(syscall: &SysCall) -> Self {
        let args = syscall.as_args();
        Self::from_array(&args)
    }

    fn from_array(args: &[usize; 8]) -> Self {
        Arguments {
            a0: args[0],
            a1: args[1],
            a2: args[2],
            a3: args[3],
            a4: args[4],
            a5: args[5],
            a6: args[6],
            a7: args[7],
        }
    }

    fn as_result(&self) -> crate::Result {
        crate::Result::from_args([self.a0, self.a1, self.a2, self.a3, self.a4, self.a5, self.a6, self.a7])
    }

    pub fn set_result(&mut self, res: &crate::Result) { *self = Self::from_array(&res.to_args()); }

    pub fn as_syscall(&self) -> core::result::Result<SysCall, crate::Error> {
        SysCall::from_args(self.a0, self.a1, self.a2, self.a3, self.a4, self.a5, self.a6, self.a7)
    }
}

/// This syscall convention passes an argument/result structure address via `r0` register.
/// Only this register has to be preserved over syscall boundary.
pub fn syscall(call: SysCall) -> SysCallResult {
    let args = Arguments::new(&call);
    let mut args_ptr = &args as *const Arguments;

    unsafe {
        asm!(
            "svc #0",
            inlateout("r0") args_ptr,
            options(nostack)
        );
    };

    let ret = unsafe { (*args_ptr).as_result() };
    match ret {
        crate::Result::Error(e) => Err(e),
        other => Ok(other),
    }
}
