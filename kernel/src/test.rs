use xous::*;
use crate::syscall;

#[test]
fn check_syscall() {
    let call = SysCall::Yield;
    syscall::handle(call);
}

#[test]
fn sanity_check() {
    return;
}
