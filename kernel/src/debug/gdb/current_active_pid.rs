use gdbstub::common::Pid;
use gdbstub::target::ext::extended_mode::CurrentActivePid;

use super::XousTarget;

// https://users.rust-lang.org/t/compile-time-const-unwrapping/51619/7
//
// This works from Rust 1.46.0 onwards, which stabilized branching and looping
// in const contexts.
macro_rules! unwrap {
    ($e:expr $(,)*) => {
        match $e {
            ::core::option::Option::Some(x) => x,
            ::core::option::Option::None => {
                ["tried to unwrap a None"][99];
                loop {}
            }
        }
    };
}

/// (Internal) The fake Pid reported to GDB when running in multi-threaded mode.
const FAKE_PID: Pid = unwrap!(Pid::new(1));

impl CurrentActivePid for XousTarget {
    fn current_active_pid(&mut self) -> Result<Pid, Self::Error> {
        Ok(self.pid.map(|v| v.into()).unwrap_or(FAKE_PID))
    }
}
