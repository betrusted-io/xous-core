// SPDX-FileCopyrightText: 2020 Sean Cross <sean@xobs.io>
// SPDX-License-Identifier: Apache-2.0

use core::fmt;

#[derive(PartialEq, Debug)]
pub enum RiscvException {
    /// When things are all 0
    NoException,

    /// 1 0
    UserSoftwareInterrupt(usize /* epc */),

    /// 1 1
    SupervisorSoftwareInterrupt(usize /* epc */),

    // [reserved]
    /// 1 3
    MachineSoftwareInterrupt(usize /* epc */),

    /// 1 4
    UserTimerInterrupt(usize /* epc */),

    /// 1 5
    SupervisorTimerInterrupt(usize /* epc */),

    // [reserved]
    /// 1 7
    MachineTimerInterrupt(usize /* epc */),

    /// 1 8
    UserExternalInterrupt(usize /* epc */),

    /// 1 9
    SupervisorExternalInterrupt(usize /* epc */),

    // [reserved]
    /// 1 11
    MachineExternalInterrupt(usize /* epc */),

    ReservedInterrupt(usize /* unknown cause number */, usize /* epc */),

    /// 0 0
    InstructionAddressMisaligned(usize /* epc */, usize /* target address */),

    /// 0 1
    InstructionAccessFault(usize /* epc */, usize /* target address */),

    /// 0 2
    IllegalInstruction(usize /* epc */, usize /* instruction value */),

    /// 0 3
    Breakpoint(usize /* epc */),

    /// 0 4
    LoadAddressMisaligned(usize /* epc */, usize /* target address */),

    /// 0 5
    LoadAccessFault(usize /* epc */, usize /* target address */),

    /// 0 6
    StoreAddressMisaligned(usize /* epc */, usize /* target address */),

    /// 0 7
    StoreAccessFault(usize /* epc */, usize /* target address */),

    /// 0 8
    CallFromUMode(usize /* epc */, usize /* ??? */),

    /// 0 9
    CallFromSMode(usize /* epc */, usize /* ??? */),

    // [reserved]
    /// 0 11
    CallFromMMode(usize /* epc */),

    /// 0 12
    InstructionPageFault(usize /* epc */, usize /* target address */),

    /// 0 13
    LoadPageFault(usize /* epc */, usize /* target address */),

    // [reserved]
    /// 0 15
    StorePageFault(usize /* epc */, usize /* target address */),

    ReservedFault(usize /* unknown cause number */, usize /* epc */, usize /* tval */),
}

impl fmt::Display for RiscvException {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use RiscvException::*;
        match *self {
            NoException => write!(f, "No trap"),
            UserSoftwareInterrupt(epc) => write!(f, "User swi from 0x{:08x}", epc),
            SupervisorSoftwareInterrupt(epc) => write!(f, "Supervisor swi from 0x{:08x}", epc),
            // --reserved--
            MachineSoftwareInterrupt(epc) => write!(f, "Machine swi at 0x{:08x}", epc),
            UserTimerInterrupt(epc) => write!(f, "User timer interrupt at 0x{:08x}", epc),
            SupervisorTimerInterrupt(epc) => {
                write!(f, "Supervisor timer interrupt at 0x{:08x}", epc)
            }
            // --reserved--
            MachineTimerInterrupt(epc) => write!(f, "Machine timer interrupt at 0x{:08x}", epc),
            UserExternalInterrupt(epc) => write!(f, "User external interrupt at 0x{:08x}", epc),
            SupervisorExternalInterrupt(epc) => {
                write!(f, "Machine external interrupt at 0x{:08x}", epc)
            }
            // --reserved--
            MachineExternalInterrupt(epc) => {
                write!(f, "Supervisor external interrupt at 0x{:08x}", epc)
            }
            ReservedInterrupt(code, epc) => {
                write!(f, "Reserved interrupt 0x{:08x} at 0x{:08x}", code, epc)
            }

            InstructionAddressMisaligned(epc, tval) => {
                write!(f, "Misaligned address instruction 0x{:08x} at 0x{:08x}", tval, epc)
            }
            InstructionAccessFault(epc, tval) => {
                write!(f, "Instruction access fault to 0x{:08x} at 0x{:08x}", tval, epc)
            }
            IllegalInstruction(epc, tval) => {
                write!(f, "Illegal instruction 0x{:08x} at 0x{:08x}", tval, epc)
            }
            Breakpoint(epc) => write!(f, "Breakpoint at 0x{:08x}", epc),
            LoadAddressMisaligned(epc, tval) => {
                write!(f, "Misaligned load address of 0x{:08x} at 0x{:08x}", tval, epc)
            }
            LoadAccessFault(epc, tval) => {
                write!(f, "Load access fault from 0x{:08x} at 0x{:08x}", tval, epc)
            }
            StoreAddressMisaligned(epc, tval) => {
                write!(f, "Misaligned store address of 0x{:08x} at 0x{:08x}", tval, epc)
            }
            StoreAccessFault(epc, tval) => {
                write!(f, "Store access fault to 0x{:08x} at 0x{:08x}", tval, epc)
            }
            CallFromUMode(epc, tval) => {
                write!(f, "Call from User mode at 0x{:08x} (???: 0x{:08x})", epc, tval)
            }
            CallFromSMode(epc, tval) => {
                write!(f, "Call from Supervisor mode at 0x{:08x} (???: 0x{:08x})", epc, tval)
            }
            // --reserved--
            CallFromMMode(epc) => write!(f, "Call from Machine mode at 0x{:08x}", epc),
            InstructionPageFault(epc, tval) => {
                write!(f, "Instruction page fault of 0x{:08x} at 0x{:08x}", tval, epc)
            }
            LoadPageFault(epc, tval) => {
                write!(f, "Load page fault of 0x{:08x} at 0x{:08x}", tval, epc)
            }
            // --reserved--
            StorePageFault(epc, tval) => {
                write!(f, "Store page fault of 0x{:08x} at 0x{:08x}", tval, epc)
            }
            ReservedFault(code, epc, tval) => {
                write!(f, "Reserved interrupt 0x{:08x} with cause 0x{:08x} at 0x{:08x}", code, tval, epc)
            }
        }
    }
}

impl RiscvException {
    pub fn from_regs(cause: usize, epc: usize, tval: usize) -> RiscvException {
        use RiscvException::*;

        if epc == 0 && tval == 0 && cause == 0 {
            return NoException;
        }

        match cause {
            0x80000000 => UserSoftwareInterrupt(epc),
            0x80000001 => SupervisorSoftwareInterrupt(epc),
            // --reserved--
            0x80000003 => MachineSoftwareInterrupt(epc),
            0x80000004 => UserTimerInterrupt(epc),
            0x80000005 => SupervisorTimerInterrupt(epc),
            // --reserved--
            0x80000007 => MachineTimerInterrupt(epc),
            0x80000008 => UserExternalInterrupt(epc),
            0x80000009 => SupervisorExternalInterrupt(epc),
            // --reserved--
            0x8000000b => MachineExternalInterrupt(epc),

            0 => InstructionAddressMisaligned(epc, tval),
            1 => InstructionAccessFault(epc, tval),
            2 => IllegalInstruction(epc, tval),
            3 => Breakpoint(epc),
            4 => LoadAddressMisaligned(epc, tval),
            5 => LoadAccessFault(epc, tval),
            6 => StoreAddressMisaligned(epc, tval),
            7 => StoreAccessFault(epc, tval),
            8 => CallFromUMode(epc, tval),
            9 => CallFromSMode(epc, tval),
            // --reserved--
            11 => CallFromMMode(epc),
            12 => InstructionPageFault(epc, tval),
            13 => LoadPageFault(epc, tval),
            // --reserved--
            15 => StorePageFault(epc, tval),
            x @ 10 | x @ 14 | x @ 16..=0x7fffffff => ReservedFault(x, epc, tval),

            x => ReservedInterrupt(x & 0x7fffffff, epc),
        }
    }
}

#[no_mangle]
pub extern "C" fn abort() -> ! {
    panic!("called abort()");
}
