use gdbstub::target;
use gdbstub::target::ext::monitor_cmd::MonitorCmd;

use super::XousTarget;

impl MonitorCmd for XousTarget {
    fn handle_monitor_cmd(
        &mut self,
        cmd: &[u8],
        mut out: target::ext::monitor_cmd::ConsoleOutput<'_>,
    ) -> Result<(), Self::Error> {
        let Ok(cmd) = core::str::from_utf8(cmd) else {
            gdbstub::outputln!(out, "command must be valid UTF-8");
            return Ok(());
        };

        if cmd.starts_with("pr") {
            if let Some(pid_str) = cmd.split_ascii_whitespace().nth(1) {
                // Parse the new PID. If it isn't a valid string, then this will be None
                let new_pid = xous_kernel::PID::new(u8::from_str_radix(pid_str, 10).unwrap_or_default());
                if let Some(previous_pid) = self.pid {
                    crate::services::SystemServices::with_mut(|system_services| {
                        system_services.resume_process_from_debug(previous_pid).unwrap()
                    });
                }
                // Disallow debugging the kernel. Sad times.
                if new_pid.map(|v| v.get() == 1).unwrap_or(false) {
                    gdbstub::outputln!(out, "Kernel cannot debug itself");
                    self.pid = None;
                    return Ok(());
                }

                self.pid = new_pid;
                if let Some(new_pid) = self.pid {
                    gdbstub::outputln!(out, "Now debugging PID {}", new_pid);
                    crate::services::SystemServices::with_mut(|system_services| {
                        system_services.pause_process_for_debug(new_pid).unwrap()
                    });
                } else {
                    gdbstub::outputln!(out, "No process is selected for debugging");
                }
                return Ok(());
            }
            gdbstub::outputln!(out, "Available processes:");

            crate::services::SystemServices::with(|system_services| {
                for process in &system_services.processes {
                    if !process.free() {
                        gdbstub::outputln!(
                            out,
                            "  {:2} {} {}",
                            process.pid,
                            if self.pid.map(|p| p == process.pid).unwrap_or(false) { '*' } else { ' ' },
                            system_services.process_name(process.pid).unwrap_or("")
                        );
                    }
                }
            });
        } else if cmd.starts_with("h") {
            gdbstub::outputln!(out, "Here is a list of help commands:");
            gdbstub::outputln!(out, "  process       Print a list of processes");
            gdbstub::outputln!(out, "  process [n]   Switch to debugging process [n]");
        } else {
            gdbstub::outputln!(out, "command not found -- try 'mon help'");
        }
        Ok(())
    }
}
