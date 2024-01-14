use gdbstub::common::{Signal, Tid};
use gdbstub::target::ext::base::multithread::MultiThreadSingleStep;

use super::XousTarget;

impl MultiThreadSingleStep for XousTarget {
    fn set_resume_action_step(&mut self, tid: Tid, _signal: Option<Signal>) -> Result<(), Self::Error> {
        // if signal.is_some() {
        //     return Err("no support for stepping with signal");
        // }
        // println!(
        //     "Performing a single step -- Setting resume action {:?} for tid {:?}",
        //     _signal, tid
        // );
        self.patch_stepi(tid)?;
        Ok(())
    }
}
