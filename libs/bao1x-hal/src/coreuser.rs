#[cfg(not(feature = "std"))]
use utralib::*;

use crate::acram::{AccessSettings, AccessType};

#[repr(usize)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum CoreuserId {
    Boot0 = 1,
    Boot1 = 2,
    Fw0 = 4,
    Fw1 = 8,
}
impl Into<&'static str> for CoreuserId {
    fn into(self) -> &'static str {
        match self {
            Self::Boot0 => "Bt0",
            Self::Boot1 => "Bt1",
            Self::Fw0 => "Fw0",
            Self::Fw1 => "Fw1",
        }
    }
}
impl From<u32> for CoreuserId {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Boot0,
            2 => Self::Boot1,
            4 => Self::Fw0,
            8 => Self::Fw1,
            _ => unreachable!("invalid coreuser one-hot integer value"),
        }
    }
}
impl CoreuserId {
    pub fn as_dense(&self) -> u32 {
        match self {
            Self::Boot0 => 0,
            Self::Boot1 => 1,
            Self::Fw0 => 2,
            Self::Fw1 => 3,
        }
    }
}

// what I want is a function that, given a DataSlotAccess, returns if access
// is allowable for a given CoreuserId
impl CoreuserId {
    pub fn is_accessible(&self, acl: &AccessSettings, access_type: &AccessType) -> bool {
        // these values are read out directly from the definition of DataSlotAccess
        let bitmask = (acl.raw_u32() >> 20) & 0xF;
        // this is a legacy of the confusion (?) in the documentation about the polarity
        // of access allowed vs not allowed.
        let as_allowed = if bitmask == 0 { 0xF } else { bitmask ^ 0x0 };

        // take advantage of the fact that our bit-coding of the enum matches
        // exactly the layout of the bit coding in the access control structure.
        let user_allowed = (*self as usize as u32 & as_allowed) != 0;

        // now compute if the requested access type is also allowed
        let access_allowed = match access_type {
            AccessType::None => true,
            AccessType::Read => acl.allows_cpu_read(),
            AccessType::Write => acl.allows_cpu_write(),
            AccessType::ReadWrite => acl.allows_cpu_read() && acl.allows_cpu_write(),
        };

        user_allowed & access_allowed
    }
}

/// This is the PID of the process that is designated to have trusted access to keys. This should be
/// the PID of the `keystore` service.
pub const TRUSTED_PID: u8 = 3;
/// This is the "user" of the trusted process. There's four regions defined which have
/// meanings in two different contexts:
///   1. Read/write control of a hardware-defined region of the main RRAM array. These are hard-defined
///      constants burned into a bank of memory that is only accessible at wafer probe, and then is sealed.
///      The main use of this r/w control is actually just to enforce read-only for the 'boot0' region, so
///      that the public keys that verify a firmware image can't be modified.
///   2. Access control of a key or data slot. These are managed by a totally different piece of hardware,
///      even though the naming terminology is the same. In the case of the RV32 CPU, the mapping of the four
///      possible regions are dynamically mapped using a LUT, from a given ASID (address space ID) to one of
///      teh four user states. An exploit chain would nominally need to gain code execution in machine mode
///      (e.g. the kernel) to modify the ASID; an ASID-mod gadget can force any process to masquerade as the
///      'trusted' process.
///
/// In the context for this constant, we are defining (2), i.e., the trusted user target for the access
/// control hardware. Note that boot0 is the default boot state of the machine - so in theory, the bootloader
/// doesn't (easily) have access to the key slots. Nominally one would need to boot into Xous and run
/// the `keystore`; but of course, in bootloader (machine mode), you have the power to conjure any ASID
/// out of thin air so with arbitrary code exec in bootloader one can bypass the controls.
///
/// Thus there are two attack surfaces of concern: the bootloader, and the kernel. Arbitrary code execution
/// in either environment is game over for the hardware access control.
pub const TRUSTED_USER: CoreuserId = CoreuserId::Fw0;
pub const LEAST_TRUSTED_USER: CoreuserId = CoreuserId::Fw1;

#[cfg(not(feature = "std"))]
pub struct Coreuser {
    csr: CSR<u32>,
}

#[cfg(not(feature = "std"))]
#[derive(Copy, Clone)]
pub struct AsidMapping {
    asid: u8,
    uid: CoreuserId,
}

/// Methods are only defined for no-std operation. In all cases, these tables should be
/// set up before Xous runs.
#[cfg(not(feature = "std"))]
impl Coreuser {
    pub fn new() -> Self { Self { csr: CSR::new(utra::coreuser::HW_COREUSER_BASE as *mut u32) } }

    /// Sets up the coreuser mappings. There's...really no options or arguments, I think this should
    /// more or less be a hard-coded table.
    pub fn set(&mut self) {
        // set to 0 so we can safely mask it later on
        self.csr.wo(utra::coreuser::USERVALUE, 0);
        self.csr.rmwf(utra::coreuser::USERVALUE_DEFAULT, CoreuserId::Fw1.as_dense());
        let trusted_asids = [
            AsidMapping { asid: 1, uid: LEAST_TRUSTED_USER }, // kernel - untrusted
            AsidMapping { asid: 2, uid: LEAST_TRUSTED_USER }, // swapper - untrusted
            AsidMapping { asid: TRUSTED_PID, uid: TRUSTED_USER }, // this is the one trusted PID
            AsidMapping { asid: 4, uid: LEAST_TRUSTED_USER }, // fillers - untrusted
            AsidMapping { asid: 5, uid: LEAST_TRUSTED_USER },
            AsidMapping { asid: 6, uid: LEAST_TRUSTED_USER },
            AsidMapping { asid: 7, uid: LEAST_TRUSTED_USER },
            AsidMapping { asid: 0, uid: CoreuserId::Boot0 }, /* this is for boot - ASID 0 is not valid in
                                                              * Xous */
        ];
        let asid_fields = [
            (utra::coreuser::MAP_LO_LUT0, utra::coreuser::USERVALUE_USER0),
            (utra::coreuser::MAP_LO_LUT1, utra::coreuser::USERVALUE_USER1),
            (utra::coreuser::MAP_LO_LUT2, utra::coreuser::USERVALUE_USER2),
            (utra::coreuser::MAP_LO_LUT3, utra::coreuser::USERVALUE_USER3),
            (utra::coreuser::MAP_HI_LUT4, utra::coreuser::USERVALUE_USER4),
            (utra::coreuser::MAP_HI_LUT5, utra::coreuser::USERVALUE_USER5),
            (utra::coreuser::MAP_HI_LUT6, utra::coreuser::USERVALUE_USER6),
            (utra::coreuser::MAP_HI_LUT7, utra::coreuser::USERVALUE_USER7),
        ];
        for (&mapping, (map_field, uservalue_field)) in trusted_asids.iter().zip(asid_fields) {
            self.csr.rmwf(map_field, mapping.asid as u32);
            self.csr.rmwf(uservalue_field, mapping.uid.as_dense());
        }
        // turn on the mappings
        self.csr.rmwf(utra::coreuser::CONTROL_ENABLE, 1);
    }

    /// Sets a "one way door" that disallows any further updating to these fields.
    pub fn protect(&mut self) {
        // map default ASID of 0 into the least trusted user (away from boot0)
        // this is necessary to ensure that the boot0 partition is write-protected
        // from this point going forward.
        self.csr.rmwf(utra::coreuser::MAP_HI_LUT7, 0);
        self.csr.rmwf(utra::coreuser::USERVALUE_USER7, LEAST_TRUSTED_USER.as_dense());

        // invert sense for Xous mode - User process 3 is trusted, the kernel is not!
        self.csr.rmwf(utra::coreuser::CONTROL_INVERT_PRIV, 1);
        self.csr.wo(utra::coreuser::PROTECT, 1);

        // ensure security bits are set according to A1 stepping
        let mut csr = CSR::new(utra::rrc::HW_RRC_BASE as *mut u32);
        csr.wo(utra::rrc::SFR_RRCCR, crate::rram::SECURITY_MODE);
    }
}
