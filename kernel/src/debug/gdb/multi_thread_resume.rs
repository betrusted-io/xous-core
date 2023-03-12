use gdbstub::common::{Signal, Tid};
use gdbstub::target::ext::base::multithread::MultiThreadResume;

use super::XousTarget;

impl MultiThreadResume for XousTarget {
    fn resume(&mut self) -> Result<(), Self::Error> {
        // println!("Resuming process {:?}", self.pid);
        // unsafe { HALTED = false };
        // match default_resume_action {
        //     ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
        //         return Err("single-stepping not supported")
        //     }
        //     _ => (),
        // }

        if let Some(pid) = self.pid {
            crate::services::SystemServices::with_mut(|system_services| {
                system_services.resume_process_from_debug(pid).unwrap()
            });
        }
        Ok(())
    }

    fn clear_resume_actions(&mut self) -> Result<(), Self::Error> {
        // println!("Clearing resume actions");
        Ok(())
    }

    fn set_resume_action_continue(
        &mut self,
        tid: Tid,
        signal: Option<Signal>,
    ) -> Result<(), Self::Error> {
        println!(
            "Setting resume action continue on process {:?} thread {:?} with signal: {:?}",
            self.pid, tid, signal
        );
        // match action {
        //     ResumeAction::Step | ResumeAction::StepWithSignal(_) => {
        //         Err("single-stepping resume action not supported")
        //     }
        //     ResumeAction::Continue | ResumeAction::ContinueWithSignal(_) => Ok(()),
        // }
        Ok(())
    }
}
