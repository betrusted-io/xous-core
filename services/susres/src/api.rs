#[cfg(target_os = "none")]
use utralib::generated::*;

pub(crate) const SERVER_NAME_SUSRES: &str     = "_Suspend/resume manager_";
pub(crate) const SERVER_NAME_EXEC_GATE: &str  = "_Suspend/resume execution gate_";

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum Opcode {
    /// requests a suspend
    SuspendRequest,

    /// register a subscriber to the suspend event
    /// (if you don't register, you just get suspend without any warning!)
    SuspendEventSubscribe,

    /// indicate we're ready to suspend
    SuspendReady,

    /// from the timeout thread
    SuspendTimeout,

    /// exit the server
    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub(crate) struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32,  // ID of the scalar message to send through (e.g. the discriminant of the Enum on the caller's side API)
    pub cid: xous::CID,   // caller-side connection ID for the scalar message to route to. Created by the caller before hooking.
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum SuspendEventCallback {
    Event, // this contains a token as well which must be returned to indicate you're ready for the suspend
    Drop,
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub(crate) enum ExecGateOpcode {
    SuspendingNow,
    Drop,
}

/////////////////////////////////////////////////////////////////////////
/////////////////////////// suspend-resume hardware management primitives
/////////////////////////////////////////////////////////////////////////
#[cfg(target_os = "none")]
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
pub enum RegOrField {
    Field(Field),
    Reg(Register),
}
#[cfg(target_os = "none")]
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
pub struct ManagedReg {
    /// register, or a field of a register
    pub item: RegOrField,
    /// override the default mask value if Some() - mask is aligned to the register or field (do not shift the mask to the left to correspond to the actual bit position)
    pub mask: Option<usize>,
    /// the saved value of the field or register
    pub value: Option<usize>,
}

//#[derive(Debug)]
#[allow(dead_code)]
#[cfg(target_os = "none")]
pub struct RegManager<const N: usize> {
    pub csr: CSR<u32>,
    pub registers: [Option<ManagedReg>; N],
    pub sus_prologue: Option<fn(&mut Self)>,
    pub sus_epilogue: Option<fn(&mut Self)>,
    pub res_prologue: Option<fn(&mut Self)>,
    pub res_epilogue: Option<fn(&mut Self)>,
}
#[allow(dead_code)]
#[cfg(target_os = "none")]
impl<const N: usize> RegManager::<N> where ManagedReg: core::marker::Copy {
    pub fn new(reg_base: *mut u32) -> RegManager::<N> {
        RegManager::<N> {
            csr: CSR::new(reg_base),
            registers: [None; N],
            sus_prologue: None,
            sus_epilogue: None,
            res_prologue: None,
            res_epilogue: None,
        }
    }
    // push registers into the manager in the order you want them suspended
    pub fn push(&mut self, mr: RegOrField, mask_override: Option<usize>) {
        let mrt = ManagedReg {
            item: mr,
            mask: mask_override,
            value: None,
        };
        for entry in self.registers.iter_mut() {
            if entry.is_none() {
                *entry = Some(mrt);
                return;
            }
        }
        // A panic! is most appropriate here, because there is really no graceful
        // remedy to this. It is a straight-up programmer error that needs to be fixed.
        panic!("Ran out of space pushing to suspend/resume manager structure. Please increase the allocated size!");
    }
}
#[cfg(target_os = "none")]
pub trait SuspendResume {
    fn suspend(&mut self);
    fn resume(&mut self);
}
#[cfg(target_os = "none")]
impl<const N: usize> SuspendResume for RegManager<N> {
    fn suspend(&mut self) {
        if let Some(sp) = self.sus_prologue {
            sp(self);
        }
        for entry in self.registers.iter_mut().rev() {
            if let Some(reg) = entry {
                // masking is done on the write side
                match reg.item {
                    RegOrField::Field(field) => reg.value = Some(self.csr.rf(field) as usize),
                    RegOrField::Reg(r) => reg.value = Some(self.csr.r(r) as usize),
                }
                *entry = Some(*reg);
            }
        }
        /*
        log::trace!("suspend csr: {:?}", self.csr);
        for entry in self.registers.iter().rev() {
            log::trace!("suspend: {:?}", entry);
        }*/
        if let Some(se) = self.sus_epilogue {
            se(self);
        }
    }
    fn resume(&mut self) {
        if let Some(rp) = self.res_prologue {
            rp(self);
        }
        for entry in self.registers.iter() { // this is in reverse order to the suspend
            if let Some(reg) = entry {
                if let Some(mut value) = reg.value {
                    if let Some(mask) = reg.mask {
                        value &= mask;
                    }
                    match reg.item {
                        RegOrField::Field(field) => self.csr.rmwf(field, value as u32),
                        RegOrField::Reg(r) => self.csr.wo(r, value as u32),
                    }
                }
            }
        }
        /*
        log::trace!("resume csr: {:?}", self.csr);
        for entry in self.registers.iter() {
            log::trace!("resume: {:?}", entry);
        }*/
        if let Some(re) = self.res_epilogue {
            re(self);
        }
    }
}

// because the volatile memory regions can be potentially large (128kiB), but fewer (maybe 5-6 total in the system),
// we allocate these as stand-alone structures and manage them explicitly.
#[cfg(target_os = "none")]
pub struct ManagedMem<const N: usize> {
    pub mem: xous::MemoryRange,
    pub backing: [u32; N],
}
#[cfg(target_os = "none")]
impl<const N: usize> SuspendResume for ManagedMem<N> {
    fn suspend(&mut self) {
        let src = self.mem.as_ptr() as *const u32;
        for words in 0..self.mem.len() {
            self.backing[words] = unsafe{src.add(words).read_volatile()};
        }
    }
    fn resume(&mut self) {
        let dst = self.mem.as_ptr() as *mut u32;
        for words in 0..self.mem.len() {
            unsafe{dst.add(words).write_volatile(self.backing[words])};
        }
    }
}




/*
// "suspend/resume u32"
struct SrU32 {
    /// offset of the register
    offset: crate::Register,
    /// the saved value of the register
    value: u32,
    /// the order in which this register should be written. If none, it's written at any convenient time.
    resume_order: Option<u8>,
    /// if this register should be in the suspend/resume set
    do_sr: bool,
}

/* sketching some structs for suspend/resume */
struct TicktimerSusres {
    csr: utralib::CSR<u32>, // new(csr_base) will allocate this register, from the virtual CSR base given to us by the owning server
    pub control: Option<u32>,
    pub time1: Option<u32>,
    pub time0: Option<u32>,
    pub resume_time1: Option<u32>,
    pub resume_time0: Option<u32>,
    pub status: Option<u32>,
    pub msleep_target1: Option<u32>,
    pub msleep_target0: Option<u32>,
    pub ev_status: Option<u32>,
    pub ev_pending: Option<u32>,
    pub ev_enable: Option<u32>,
    pub ticktimer_irq: Option<bool>, // whether to enable or not after resume
    pub wait_suspend: Option<fn(timeout: u32) -> bool>, // function to call to check if a block can suspend
}
pub trait SuspendResume {
    pub fn suspend(&mut self) {
        /*
        go through each item of the structure, and if the item is Some(item), access the corresponding
        register and save it into the register
         */
    }
    pub fn resume(&mut self) {
        /*
        go through each item of the stucture, and if the item is Some(item), unwrap the value and
        poke it into the register
        */
    }
}

enum TickttimerReg {
    Control = 0,
    Time1 = 1,
    Time0 = 2,
    MSleepTarget1 = 3,
    //...
}
//enum TickttimerInterrupt {
//    Alarm = 0,
//}

struct ManagedReg {
    offset: TickttimerReg,
    mask: usize, // mask of data to store
    value: u32,
}
//struct ManagedInterrupt {
//    irq: usize,
//}
struct ManagedMem {
    offset: usize, // the starting offset of the volatile memory block
    mem: [u32; MEMLEN], // backup copy of the memory
}
// this can operate on the above three structure
pub trait SusResOps {
    fn save();
    fn restore();
}

/*
Interrupts need no suspend/resume setup:
- the hooks and callback locations are all stored in non-volatile SRAM
- the SIM register, which contains the mask of interrupts that are enabled, will be restored
  by the loader on boot

Between these two facts, all of the interrupt mechanism are transparently handled by the kernel
 */


struct SusResManager {
    csr: utralib::CSR<u32>,
    pub registers: [Option<ManagedReg>; 12], // max number is set by the utra2svd generator
    //pub interrupts: [Option<usize>; 1], // if in this list, this interrupt should be enabled

    // i think maybe these go in the library, not in the auto-generated manager structure
    pub suspend_cb: fn(),
    pub resume_cb: fn(),
}
pub trait SusResBlockOps {
    fn append(reg: ManagedReg); // adds a register to the managed suspend list
    fn suspend(); // iterates through the list of registers and performs the suspend operation
    fn resume(); // iterates through the list of registers and performs the resume operation
}
*/