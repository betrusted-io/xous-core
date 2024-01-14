use core::convert::TryInto;
use core::hint::unreachable_unchecked;

// mod disasm;

use super::XousTarget;
use gdbstub::common::Tid;
use gdbstub::target::ext::base::multithread::MultiThreadBase;
use gdbstub::target::ext::base::single_register_access::SingleRegisterAccess;
use gdbstub::target::Target;
use gdbstub_arch::riscv::reg::id::RiscvRegId;

enum Opcode {
    Opcode16(u16),
    Opcode32(u32),
}

impl core::fmt::LowerHex for Opcode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Opcode::Opcode16(val) => write!(f, "{:04x}", val),
            Opcode::Opcode32(val) => write!(f, "{:08x}", val),
        }
    }
}

pub(crate) struct PatchedInstruction {
    /// The address that was patched
    pc: u32,

    /// The value of the region before it was patched
    previous: Opcode,
}

pub(crate) struct XousTargetInner {
    /// When doing a `stepi` we patch the instruction with an illegal instruction
    /// and store the previous value here.
    step_patch: Option<PatchedInstruction>,
}

impl Default for XousTargetInner {
    fn default() -> Self {
        XousTargetInner { step_patch: None }
    }
}

#[derive(Debug)]
enum OpcodeType {
    Rv16,
    Rv32,
}

/// GDB is broken and will send `vCont;s` to targets even if they
/// report that they don't support single-stepping. Therefore,
/// we have to include a partial disassembler in the kernel in
/// order to work around this bug.

/// Patch the program at the given process and thread such that
/// the next instruction it executes is `c.break`.
///
/// Return the previous memory value so that it can be saved.
impl XousTarget {
    pub fn patch_stepi(&mut self, tid: Tid) -> Result<(), <XousTarget as Target>::Error> {
        if self.inner.step_patch.is_some() {
            self.unpatch_stepi(tid)?;
        }

        let mut pc = [0u8; core::mem::size_of::<u32>()];
        self.read_register(tid, RiscvRegId::Pc, &mut pc)
            .or(Err("unable to read register"))?;
        let pc = u32::from_le_bytes(pc);

        let mut opcode = [0u8; core::mem::size_of::<u32>()];
        self.read_addrs(pc, &mut opcode, tid)
            .or(Err("unable to read memory"))?;
        let current = u32::from_le_bytes(opcode);

        let current_opcode_type = match current & 0b11 {
            0b00 | 0b01 | 0b10 => OpcodeType::Rv16,
            0b11 => OpcodeType::Rv32,
            _ => unsafe { unreachable_unchecked() },
        };

        let new_pc = match current_opcode_type {
            OpcodeType::Rv16 => self.next_pc_16(pc, (current & 0xffff).try_into().unwrap(), tid)?,
            OpcodeType::Rv32 => self.next_pc_32(pc, current, tid)?,
        };

        let mut opcode = [0u8; core::mem::size_of::<u32>()];
        self.read_addrs(new_pc, &mut opcode, tid)
            .or(Err("unable to read memory"))?;
        let existing = u32::from_le_bytes(opcode);

        let existing_opcode_type = match existing & 0b11 {
            0b00 | 0b01 | 0b10 => OpcodeType::Rv16,
            0b11 => OpcodeType::Rv32,
            _ => unsafe { unreachable_unchecked() },
        };

        match existing_opcode_type {
            OpcodeType::Rv16 => {
                self.next_pc_16(new_pc, (existing & 0xffff).try_into().unwrap(), tid)?
            }
            OpcodeType::Rv32 => self.next_pc_32(new_pc, existing, tid)?,
        };

        let (existing, new_opcode) = match existing_opcode_type {
            OpcodeType::Rv16 => (Opcode::Opcode16(existing as u16), Opcode::Opcode16(0x9002)), // c.ebreak
            OpcodeType::Rv32 => (Opcode::Opcode32(existing), Opcode::Opcode32(0x0010_0073)), // c.ebreak c.ebreak
        };

        match new_opcode {
            Opcode::Opcode16(val) => self
                .write_addrs(new_pc, &val.to_le_bytes(), tid)
                .or(Err("unable to write memory"))?,
            Opcode::Opcode32(val) => self
                .write_addrs(new_pc, &val.to_le_bytes(), tid)
                .or(Err("unable to write memory"))?,
        }

        unsafe {
            core::arch::asm!(
                "
            fence.i
            fence
        "
            )
        };

        assert!(self.inner.step_patch.is_none());
        self.inner.step_patch = Some(PatchedInstruction {
            pc: new_pc,
            previous: existing,
        });

        Ok(())
    }

    pub fn unpatch_stepi(&mut self, tid: Tid) -> Result<(), <XousTarget as Target>::Error> {
        let Some(step_patch) = self.inner.step_patch.take() else {
            return Ok(());
        };
        crate::services::SystemServices::with(|system_services| {
            let current_pid = system_services.current_pid();

            // Actiavte the debugging process and iterate through it,
            // noting down each active thread.
            let debugging_pid = self.pid.unwrap();
            system_services
                .get_process(debugging_pid)
                .unwrap()
                .activate()
                .unwrap();

            match step_patch.previous {
                Opcode::Opcode16(val) => self
                    .write_addrs(step_patch.pc, &val.to_le_bytes(), tid)
                    .or(Err("unable to undo patch"))?,
                Opcode::Opcode32(val) => self
                    .write_addrs(step_patch.pc, &val.to_le_bytes(), tid)
                    .or(Err("unable to undo patch"))?,
            }
            // Restore the previous PID
            system_services
                .get_process(current_pid)
                .unwrap()
                .activate()
                .unwrap();
            Ok(())
        })?;
        unsafe {
            core::arch::asm!(
                "
            fence.i
            fence
        "
            )
        };
        Ok(())
    }

    fn next_pc_16(
        &mut self,
        pc: u32,
        opcode: u16,
        tid: Tid,
    ) -> Result<u32, <XousTarget as Target>::Error> {
        let opcode = opcode as u32;
        if opcode & 0b1110_00000_1111111 == 0b1000_00000_0000010 {
            // c.jr or c.jalr
            let rs1 = (opcode >> 7) & 0b11111;
            if rs1 == 0 {
                return Ok(pc + 2);
            }
            let mut rs = [0u8; 4];
            self.read_register(tid, RiscvRegId::Gpr(rs1 as u8), &mut rs)
                .or(Err("unable to read register"))?;
            let rs1_val = u32::from_le_bytes(rs);
            Ok(rs1_val)
        } else if opcode & 0b011_0000000000011 == 0b001_00000000000_01 {
            // c.j  or c.jal
            // [11|4|9:8|10|6|7|3:1|5]
            let mut imm = (((opcode >> (3 - 1)) & 0b00000001110)
                | (opcode >> (11 - 4)) & 0b00000010000
                | (opcode << (5 - 2)) & 0b00000100000
                | (opcode >> (7 - 6)) & 0b00001000000
                | (opcode << (7 - 6)) & 0b00010000000
                | (opcode >> (9 - 8)) & 0b01100000000
                | (opcode << (10 - 8)) & 0b10000000000) as u32;
            // Sign extend
            if opcode & 0b0001_0000_0000_0000 != 0 {
                imm |= 0xffff_f800;
            }

            Ok(pc.wrapping_add(imm))
        } else if opcode & 0b110_00000000000_11 == 0b110_00000000000_01 {
            // c.bnez  or c.beqz
            let rs1 = ((opcode >> 7) & 0b111) | 0b1000;
            let mut rs1_val = [0u8; 4];
            self.read_register(tid, RiscvRegId::Gpr(rs1 as u8), &mut rs1_val)
                .or(Err("unable to read register"))?;
            let rs1_val = u32::from_le_bytes(rs1_val);

            let mut imm = (((opcode >> 2) & 0b00_0_00_11_0)
                | (opcode >> (10 - 3)) & 0b00_0_11_00_0
                | (opcode << (5 - 2)) & 0b00_1_00_00_0
                | (opcode << (7 - 6)) & 0b11_0_00_00_0) as u32;
            if opcode & (1 << 12) != 0 {
                imm |= 0xffff_ff00;
            }

            if opcode & 0b001_00000000000_00 == 0b001_00000000000_00 {
                let target = if rs1_val != 0 {
                    pc.wrapping_add(imm)
                } else {
                    pc + 2
                };
                Ok(target)
            } else {
                let target = if rs1_val == 0 {
                    pc.wrapping_add(imm)
                } else {
                    pc + 2
                };
                Ok(target)
            }
        } else {
            Ok(pc.wrapping_add(2))
        }
    }

    fn next_pc_32(
        &mut self,
        pc: u32,
        opcode: u32,
        tid: Tid,
    ) -> Result<u32, <XousTarget as Target>::Error> {
        // We probably also ought to look for an LR sequence,
        // but that seems complicated.

        // jal:  xxxxxxxxxxxxxxxxxxxxxxxxx1101111
        if opcode & 0b1111111 == 0b110_1111 {
            let mut imm = ((opcode >> 20) & 0b0_1111111111_0)
                | ((opcode >> 10) & 0b1_0000000000_0)
                | (opcode & 0b11111111_0000000000_0);
            if opcode & 0x80000000 != 0 {
                imm |= 0xfff8_0000;
            }
            Ok(pc.wrapping_add(imm))
        }
        // jalr: xxxxxxxxxxxxxxxxx000xxxxx1100111
        else if opcode & 0b000000000000_00000_111_00000_1111111
            == 0b000000000000_00000_000_00000_1100111
        {
            let mut imm = (opcode >> 20) & 0b111_1111_1111;
            if opcode & 0x80000000 != 0 {
                imm |= 0xffff_f800;
            }

            let rs1 = (opcode >> 15) & 0b11111;
            let mut rs1_val = [0u8; 4];
            self.read_register(tid, RiscvRegId::Gpr(rs1 as u8), &mut rs1_val)
                .or(Err("unable to read register"))?;
            let rs1_val = u32::from_le_bytes(rs1_val);

            Ok(rs1_val.wrapping_add(imm))
        } else if opcode & 0b1111111 == 0b1100011 {
            let mut imm = ((opcode >> 7) & 0b0_000000_1111_0)
                | (opcode >> 20) & 0b0_111111_0000_0
                | (opcode << 4) & 0b1_000000_0000_0;
            if opcode & 0x80000000 != 0 {
                imm |= 0xffff_f000;
            }

            let rs1 = (opcode >> 15) & 0b11111;
            let mut rs = [0u8; 4];
            self.read_register(tid, RiscvRegId::Gpr(rs1 as u8), &mut rs)
                .or(Err("unable to read register"))?;
            let rs1_val = i32::from_le_bytes(rs);

            let rs2 = (opcode >> 20) & 0b11111;
            let mut rs = [0u8; 4];
            self.read_register(tid, RiscvRegId::Gpr(rs2 as u8), &mut rs)
                .or(Err("unable to read register"))?;
            let rs2_val = i32::from_le_bytes(rs);

            Ok(pc.wrapping_add(match (opcode >> 12) & 0b111 {
                // beq
                0b000 => {
                    if rs1_val == rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                // bne
                0b001 => {
                    if rs1_val != rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                // blt
                0b100 => {
                    if rs1_val < rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                // bge
                0b101 => {
                    if rs1_val >= rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                // bltu
                0b110 => {
                    if rs1_val < rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                // bgeu
                0b111 => {
                    if rs1_val >= rs2_val {
                        imm
                    } else {
                        4
                    }
                }
                _ => pc + 4,
            }))
        } else {
            Ok(pc.wrapping_add(4))
        }
    }
}
