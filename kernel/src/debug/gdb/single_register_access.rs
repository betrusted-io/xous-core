use core::convert::TryInto;

use gdbstub::common::Tid;
use gdbstub::target::ext::base::single_register_access::SingleRegisterAccess;
use gdbstub::target::{TargetError, TargetResult};
use gdbstub_arch::riscv::reg::id::RiscvRegId;

use super::XousTarget;

impl SingleRegisterAccess<Tid> for XousTarget {
    fn read_register(
        &mut self,
        tid: Tid,
        reg_id: RiscvRegId<u32>,
        buf: &mut [u8],
    ) -> TargetResult<usize, Self> {
        // For the case of no PID, fake a value
        let Some(pid) = self.pid else {
            buf.copy_from_slice(&0u32.to_le_bytes());
            return Ok(buf.len());
        };

        let reg_id = match reg_id {
            RiscvRegId::Gpr(0) => {
                buf.copy_from_slice(&0u32.to_le_bytes());
                return Ok(buf.len());
            }
            RiscvRegId::Gpr(x) => x as usize - 1,
            RiscvRegId::Pc => 31,
            _ => {
                return Err(TargetError::Fatal("register out of range"));
            }
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();
            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
            system_services.get_process(debugging_pid).unwrap().activate().unwrap();
            let process = crate::arch::process::Process::current();
            let thread = process.thread(tid.get());
            let reg = if reg_id == 31 {
                &thread.sepc
            } else {
                match thread.registers.get(reg_id) {
                    Some(val) => val,
                    None => return Err(TargetError::Fatal("register out of range")),
                }
            };

            buf.copy_from_slice(&reg.to_le_bytes());

            // Restore the previous PID
            system_services.get_process(current_pid).unwrap().activate().unwrap();
            Ok(buf.len())
        })
    }

    fn write_register(&mut self, tid: Tid, reg_id: RiscvRegId<u32>, val: &[u8]) -> TargetResult<(), Self> {
        let Some(pid) = self.pid else { return Ok(()) };

        let w = u32::from_le_bytes(val.try_into().map_err(|_| TargetError::Fatal("invalid data"))?);

        let reg_id = match reg_id {
            RiscvRegId::Gpr(0) => {
                return Ok(());
            }
            RiscvRegId::Gpr(x) => x as usize - 1,
            RiscvRegId::Pc => 31,
            _ => {
                return Err(TargetError::Fatal("register out of range"));
            }
        };

        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = pid;
            system_services.get_process(debugging_pid).unwrap().activate().unwrap();
            let mut process = crate::arch::process::Process::current();
            let thread = process.thread_mut(tid.get());
            let reg = if reg_id == 31 {
                &mut thread.sepc
            } else {
                match thread.registers.get_mut(reg_id) {
                    Some(val) => val,
                    None => return Err(TargetError::Fatal("register out of range")),
                }
            };
            *reg = w as usize;

            // Restore the previous PID
            system_services.get_process(current_pid).unwrap().activate().unwrap();
            Ok(())
        })
    }
}
