#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(usize)]
pub enum ExceptionType {
    InstructionAddressMisaligned = 0,
    InstructionAccessFault = 1,
    IllegalInstruction = 2,
    LoadAddressMisaligned = 3,
    LoadAccessFault = 4,
    StoreAddressMisaligned = 5,
    StoreAccessFault = 6,
    InstructionPageFault = 7,
    LoadPageFault = 8,
    StorePageFault = 9,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Exception {
    InstructionAddressMisaligned(usize /* epc */, usize /* addr */),
    InstructionAccessFault(usize /* epc */, usize /* addr */),
    IllegalInstruction(usize /* epc */, usize /* instruction */),
    LoadAddressMisaligned(usize /* epc */, usize /* addr */),
    LoadAccessFault(usize /* epc */, usize /* addr */),
    StoreAddressMisaligned(usize /* epc */, usize /* addr */),
    StoreAccessFault(usize /* epc */, usize /* addr */),
    InstructionPageFault(usize /* epc */, usize /* addr */),
    LoadPageFault(usize /* epc */, usize /* addr */),
    StorePageFault(usize /* epc */, usize /* addr */),
    Unknown(usize, usize, usize),
}

impl Exception {
    pub fn new(a0: usize, a1: usize, a2: usize) -> Exception {
        match a0 {
            0 /*ExceptionType::InstructionAddressMisaligned as usize*/ => {
                Exception::InstructionAddressMisaligned(a1, a2)
            }
            1 /*ExceptionType::InstructionAccessFault as usize*/ => {
                Exception::InstructionAccessFault(a1, a2)
            }
            2 /*ExceptionType::IllegalInstruction as usize*/ => Exception::IllegalInstruction(a1, a2),
            3 /*ExceptionType::LoadAddressMisaligned as usize*/ => Exception::LoadAddressMisaligned(a1, a2),
            4 /*ExceptionType::LoadAccessFault as usize*/ => Exception::LoadAccessFault(a1, a2),
            5 /*ExceptionType::StoreAddressMisaligned as usize*/ => {
                Exception::StoreAddressMisaligned(a1, a2)
            }
            6 /*ExceptionType::StoreAccessFault as usize*/ => Exception::StoreAccessFault(a1, a2),
            7 /*ExceptionType::InstructionPageFault as usize*/ => Exception::InstructionPageFault(a1, a2),
            8 /*ExceptionType::LoadPageFault as usize*/ => Exception::LoadPageFault(a1, a2),
            9 /*ExceptionType::StorePageFault as usize*/ => Exception::StorePageFault(a1, a2),
            _ => Exception::Unknown(a0, a1, a2),
        }
    }

    pub fn pc(&self) -> usize {
        match *self {
            Exception::InstructionAddressMisaligned(pc, _)
            | Exception::InstructionAccessFault(pc, _)
            | Exception::IllegalInstruction(pc, _)
            | Exception::LoadAddressMisaligned(pc, _)
            | Exception::LoadAccessFault(pc, _)
            | Exception::StoreAddressMisaligned(pc, _)
            | Exception::StoreAccessFault(pc, _)
            | Exception::InstructionPageFault(pc, _)
            | Exception::LoadPageFault(pc, _)
            | Exception::StorePageFault(pc, _)
            | Exception::Unknown(_, pc, _) => pc,
        }
    }

    pub fn address(&self) -> Option<usize> {
        match *self {
            Exception::InstructionAddressMisaligned(_, address)
            | Exception::InstructionAccessFault(_, address)
            | Exception::LoadAddressMisaligned(_, address)
            | Exception::LoadAccessFault(_, address)
            | Exception::StoreAddressMisaligned(_, address)
            | Exception::StoreAccessFault(_, address)
            | Exception::InstructionPageFault(_, address)
            | Exception::LoadPageFault(_, address)
            | Exception::StorePageFault(_, address) => Some(address),
            _ => None,
        }
    }
}
