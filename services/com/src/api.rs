use xous::{Message, ScalarMessage};
use xous_ipc::String;

// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
use com_rs::*;
use com_rs_ref as com_rs;

pub(crate) const SERVER_NAME_COM: &str      = "_COM manager_";

#[derive(Debug, Default, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BattStats {
    /// instantaneous voltage in mV
    pub voltage: u16,
    /// state of charge in %, as inferred by impedance tracking
    pub soc: u8,
    /// instantaneous current draw in mA
    pub current: i16,
    /// remaining capacity in mA, as measured by coulomb counting
    pub remaining_capacity: u16,
}

impl From<[usize; 2]> for BattStats {
    fn from(a: [usize; 2]) -> BattStats {
        BattStats {
            voltage: (a[0] & 0xFFFF) as u16,
            soc: ((a[0] >> 16) & 0xFF) as u8,
            current: ((a[1] >> 16) & 0xFFFF) as i16,
            remaining_capacity: (a[1] & 0xFFFF) as u16,
        }
    }
}

impl Into<[usize; 2]> for BattStats {
    fn into(self) -> [usize; 2] {
        [
            (self.voltage as usize & 0xffff) | ((self.soc as usize) << 16) & 0xFF_0000,
            (self.remaining_capacity as usize & 0xffff)
                | ((self.current as usize) << 16) & 0xffff_0000,
        ]
    }
}
#[allow(dead_code)]
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// Battery stats
    BattStats,

    /// Battery stats, non-blocking
    BattStatsNb,

    /// Query Full charge capacity of the battery
    //BattFullCapacity,

    /// Turn Boost Mode On
    //BoostOn,

    /// Turn Boost Mode Off
    //BoostOff,

    /// Read the current accelerations off the IMU
    //ImuAccelRead,

    /// Power off the SoC
    PowerOffSoc,

    /// Ship mode (battery disconnect)
    //ShipMode,

    /// Is the battery charging?
    //IsCharging,

    /// Set the backlight brightness
    //SetBackLight,

    /// Request charging
    //RequestCharging,

    /// Erase a region of EC FLASH
    //FlashErase,

    /// Program a page of FLASH
    //FlashProgram(&'a [u8]),

    /// Update the SSID list
    //SsidScan,

    /// Return the latest SSID list
    //SsidFetch,

    /// Fetch the git ID of the EC
    EcGitRev,

    /// Fetch the firmware rev of the WF200
    Wf200Rev,

    /// Send a line of PDS data
    Wf200PdsLine, //String<512>

    /// request for a listener to BattStats events
    RegisterBattStatsListener, //String<64>
}

/// These enums indicate what kind of callback type we're sending.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Callback {
    /// Battery status
    BattStats,
    /// Server is quitting, drop connections
    Drop,
}
