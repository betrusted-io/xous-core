use gdbstub::common::{Signal, Tid};
use gdbstub::target::ext::base::multithread::{MultiThreadResume, MultiThreadSingleStepOps};

use super::XousTarget;

impl MultiThreadResume for XousTarget {
    fn resume(&mut self) -> Result<(), Self::Error> {
        if let Some(pid) = self.pid {
            crate::services::SystemServices::with_mut(|system_services| {
                system_services.resume_process_from_debug(pid).unwrap()
            });
        }
        Ok(())
    }

    fn support_single_step(&mut self) -> Option<MultiThreadSingleStepOps<'_, Self>> { Some(self) }

    fn clear_resume_actions(&mut self) -> Result<(), Self::Error> { Ok(()) }

    fn set_resume_action_continue(&mut self, _tid: Tid, _signal: Option<Signal>) -> Result<(), Self::Error> {
        Ok(())
    }
}
