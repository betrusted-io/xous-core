use core::convert::TryInto;

use gdbstub::common::{Pid, Tid};
use gdbstub::target;
use gdbstub::target::ext::extended_mode::{AttachKind, ExtendedMode, ShouldTerminate};
use gdbstub::target::{TargetError, TargetResult};

use super::XousTarget;

impl ExtendedMode for XousTarget {
    fn attach(&mut self, new_pid: Pid) -> TargetResult<(), Self> {
        if let Some(previous_pid) = self.pid.take() {
            self.unpatch_stepi(Tid::new(1).unwrap()).ok();
            crate::services::SystemServices::with_mut(|system_services| {
                system_services.resume_process_from_debug(previous_pid).unwrap()
            });
        }

        // Disallow debugging the kernel. Sad times.
        if new_pid.get() == 1 {
            println!("Kernel cannot debug itself");
            self.pid = None;
            return Err(TargetError::NonFatal);
        }

        self.pid = new_pid.try_into().map(|v| Some(v)).unwrap_or(None);
        if let Some(pid) = self.pid {
            crate::services::SystemServices::with_mut(|system_services| {
                system_services
                    .pause_process_for_debug(pid)
                    .map_err(|e| {
                        println!("PID {} not found", new_pid);
                        e
                    })
                    .and_then(|v| {
                        println!("Now debugging PID {}", new_pid);
                        Ok(v)
                    })
                    .ok();
            });
        } else {
            println!("Invalid PID specified");
        }
        Ok(())
    }

    fn kill(&mut self, pid: Option<Pid>) -> TargetResult<ShouldTerminate, Self> {
        println!("GDB sent a kill request for pid {:?}", pid);
        Ok(ShouldTerminate::No)
    }

    fn restart(&mut self) -> Result<(), Self::Error> {
        println!("GDB sent a restart request");
        Ok(())
    }

    fn query_if_attached(&mut self, _pid: Pid) -> TargetResult<AttachKind, Self> { Ok(AttachKind::Attach) }

    fn run(
        &mut self,
        _filename: Option<&[u8]>,
        _args: target::ext::extended_mode::Args<'_, '_>,
    ) -> TargetResult<Pid, Self> {
        println!("Trying to run command (?!)");
        Err(TargetError::NonFatal)
    }

    #[inline(always)]
    fn support_current_active_pid(
        &mut self,
    ) -> Option<target::ext::extended_mode::CurrentActivePidOps<'_, Self>> {
        Some(self)
    }
}
