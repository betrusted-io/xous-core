// SPDX-FileCopyrightText: 2022 Foundation Devices, Inc <hello@foundationdevices.com>
// SPDX-License-Identifier: Apache-2.0

use crate::services::Thread;

extern "C" {
    fn _resume_trampoline(mode: usize, thread: &Thread, enable_irqs: bool) -> !;
}

pub fn invoke(
    thread: &mut crate::arch::arm::process::Thread,
    _supervisor: bool,
    pc: usize,
    sp: usize,
    ret_addr: usize,
    args: &[usize],
) {
    assert!(args.len() <= 4, "too many arguments to invoke()");

    thread.resume_addr = pc;
    thread.sp = sp;

    if let Some(arg) = args.get(0) {
        thread.r0 = *arg;
    }
    if let Some(arg) = args.get(1) {
        thread.r1 = *arg;
    }
    if let Some(arg) = args.get(2) {
        thread.r2 = *arg;
    }
    if let Some(arg) = args.get(3) {
        thread.r3 = *arg;
    }

    thread.ret_addr = ret_addr;
}

pub fn resume(supervisor: bool, thread: &Thread) -> ! {
    resume_inner(supervisor, thread, true)
}

pub fn resume_no_irqs(supervisor: bool, thread: &Thread) -> ! {
    resume_inner(supervisor, thread, false)
}

fn resume_inner(supervisor: bool, thread: &Thread, enable_irqs: bool) -> ! {
    // Restore thread stack and PC, pass resume arguments via r0-r4
    klog!("resume: setting sp={:08x}, res_addr={:08x}, lr={:08x} | privileged: {:?} | irqs: {:?} | ret: {:08x}", thread.sp, thread.resume_addr, thread.lr, supervisor, enable_irqs, thread.ret_addr);

    // See ARM ARM
    // B1.3.1 ARM processor modes
    let mode: usize = if supervisor {
        0b11111  // System mode
    } else {
        0b10000  // User mode
    };

    unsafe {
        _resume_trampoline(mode, thread, enable_irqs);
    }
}
