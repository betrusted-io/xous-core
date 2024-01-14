#![cfg_attr(not(target_os = "none"), allow(dead_code))]

#[cfg(any(feature = "precursor", feature = "renode"))]
use utralib::generated::*;

pub const SERVER_NAME_SUSRES: &str = "_Suspend/resume manager_";

/// Note: there must be at least one subscriber to the `Last` suspend order event, otherwise
/// the logic will never terminate. There may be multiple `Last` subscribers, but the order at
/// which they finish would be indeterminate. Currently, the `Last` subscriber is the `spinor`
/// block, which is last because you want to make sure all the PDDB commits and other saved
/// data are flushed before turning off access to the SPINOR.
#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone, PartialEq, Eq)]
pub enum SuspendOrder {
    Early,
    Normal,
    Late,
    Later,
    Last,
}
impl SuspendOrder {
    pub fn next(&self) -> SuspendOrder {
        match self {
            SuspendOrder::Early => SuspendOrder::Normal,
            SuspendOrder::Normal => SuspendOrder::Late,
            SuspendOrder::Late => SuspendOrder::Later,
            SuspendOrder::Later => SuspendOrder::Last,
            SuspendOrder::Last => SuspendOrder::Last,
        }
    }
}

#[derive(num_derive::FromPrimitive, num_derive::ToPrimitive, Debug)]
pub enum Opcode {
    /// requests a suspend
    SuspendRequest,
    /// locks out suspend capability -- meant to be used only to prevent suspend during firmware updates
    SuspendDeny,
    /// unlocks suspend capability
    SuspendAllow,

    /// register a subscriber to the suspend event
    /// (if you don't register, you just get suspend without any warning!)
    SuspendEventSubscribe,

    /// indicate we're ready to suspend
    SuspendReady,

    /// from the timeout thread
    SuspendTimeout,

    /// queries if my suspend was clean or not
    WasSuspendClean,

    /// reboot opcodes
    RebootRequest,
    RebootSocConfirm, // all peripherals + CPU
    RebootCpuConfirm, // just the CPU, peripherals (in particular the USB debug bridge) keep state

    /// not tested - reboot address
    RebootVector, //(u32),

    /// used by processes to indicate they are suspending now; this blocks until resume using the "execution
    /// gate"
    SuspendingNow,

    /// used to power off the system without suspend
    PowerOff,

    /// exit the server
    Quit,
}

#[derive(Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Copy, Clone)]
pub struct ScalarHook {
    pub sid: (u32, u32, u32, u32),
    pub id: u32, /* ID of the scalar message to send through (e.g. the discriminant of the Enum on the
                  * caller's side API) */
    pub cid: xous::CID, /* caller-side connection ID for the scalar message to route to. Created by the
                         * caller before hooking. */
    pub order: SuspendOrder,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub enum SuspendEventCallback {
    Event, // this contains a token as well which must be returned to indicate you're ready for the suspend
    Drop,
}

/////////////////////////////////////////////////////////////////////////
/////////////////////////// suspend-resume hardware management primitives
/////////////////////////////////////////////////////////////////////////
#[cfg(any(feature = "precursor", feature = "renode"))]
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
pub enum RegOrField {
    Field(Field),
    Reg(Register),
}
#[cfg(any(feature = "precursor", feature = "renode"))]
#[allow(dead_code)]
#[derive(Debug, Copy, Clone)]
pub struct ManagedReg {
    /// register, or a field of a register
    pub item: RegOrField,
    /// override the default mask value if Some() - mask is aligned to the register or field (do not shift
    /// the mask to the left to correspond to the actual bit position)
    pub mask: Option<usize>,
    /// the saved value of the field or register
    pub value: Option<usize>,
    /// if true, value is static and should not be updated during suspend, but restored to this value ta
    /// resume
    pub fixed_value: bool,
}

#[derive(Debug)]
#[allow(dead_code)]
#[cfg(any(feature = "precursor", feature = "renode"))]
pub struct RegManager<const N: usize> {
    pub csr: CSR<u32>,
    pub registers: [Option<ManagedReg>; N],
}
#[allow(dead_code)]
#[cfg(any(feature = "precursor", feature = "renode"))]
impl<const N: usize> RegManager<N>
where
    ManagedReg: core::marker::Copy,
{
    pub fn new(reg_base: *mut u32) -> RegManager<N> {
        RegManager::<N> { csr: CSR::new(reg_base), registers: [None; N] }
    }

    pub fn push_fixed_value(&mut self, mr: RegOrField, value: usize) {
        let mrt = ManagedReg { item: mr, mask: None, value: Some(value), fixed_value: true };
        for entry in self.registers.iter_mut() {
            if entry.is_none() {
                *entry = Some(mrt);
                return;
            }
        }
        panic!(
            "Ran out of space pushing to suspend/resume manager structure. Please increase the allocated size!"
        );
    }

    // push registers into the manager in the order that you would normally initialized them
    pub fn push(&mut self, mr: RegOrField, mask_override: Option<usize>) {
        let mrt = ManagedReg { item: mr, mask: mask_override, value: None, fixed_value: false };
        for entry in self.registers.iter_mut() {
            if entry.is_none() {
                *entry = Some(mrt);
                return;
            }
        }
        // A panic! is most appropriate here, because there is really no graceful
        // remedy to this. It is a straight-up programmer error that needs to be fixed.
        panic!(
            "Ran out of space pushing to suspend/resume manager structure. Please increase the allocated size!"
        );
    }
}
#[cfg(any(feature = "precursor", feature = "renode"))]
pub trait SuspendResume {
    fn suspend(&mut self);
    fn resume(&mut self);
}
#[cfg(any(feature = "precursor", feature = "renode"))]
impl<const N: usize> SuspendResume for RegManager<N> {
    fn suspend(&mut self) {
        for entry in self.registers.iter_mut().rev() {
            if let Some(reg) = entry {
                if !reg.fixed_value {
                    // masking is done on the write side
                    match reg.item {
                        RegOrField::Field(field) => reg.value = Some(self.csr.rf(field) as usize),
                        RegOrField::Reg(r) => reg.value = Some(self.csr.r(r) as usize),
                    }
                    *entry = Some(*reg);
                }
            }
        }
        /*
        log::trace!("suspend csr: {:?}", self.csr);
        for entry in self.registers.iter().rev() {
            log::trace!("suspend: {:?}", entry);
        }*/
    }

    fn resume(&mut self) {
        for entry in self.registers.iter() {
            // this is in reverse order to the suspend
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
    }
}

// because the volatile memory regions can be potentially large (128kiB), but fewer (maybe 5-6 total in the
// system), we allocate these as stand-alone structures and manage them explicitly.
#[derive(Debug)]
#[cfg(any(feature = "precursor", feature = "renode"))]
pub struct ManagedMem<const N: usize> {
    pub mem: xous::MemoryRange,
    pub backing: [u32; N],
}
#[allow(dead_code)]
#[cfg(any(feature = "precursor", feature = "renode"))]
impl<const N: usize> ManagedMem<N> {
    pub fn new(src: xous::MemoryRange) -> Self { ManagedMem { mem: src, backing: [0; N] } }
}
#[cfg(any(feature = "precursor", feature = "renode"))]
impl<const N: usize> SuspendResume for ManagedMem<N> {
    fn suspend(&mut self) {
        let src = self.mem.as_ptr() as *const u32;
        for words in 0..(N / core::mem::size_of::<u32>()) {
            self.backing[words] = unsafe { src.add(words).read_volatile() };
        }
    }

    fn resume(&mut self) {
        let dst = self.mem.as_ptr() as *mut u32;
        for words in 0..(N / core::mem::size_of::<u32>()) {
            unsafe { dst.add(words).write_volatile(self.backing[words]) };
        }
    }
}
