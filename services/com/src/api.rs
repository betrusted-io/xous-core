// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
pub(crate) const SERVER_NAME_COM: &str      = "_COM manager_";
pub use com_rs_ref::serdes::Ipv4Conf;
#[allow(dead_code)]
pub const WF200_PASS_MAX_LEN: usize = 64;
#[allow(dead_code)]
pub const WF200_SSID_MAX_LEN: usize = 32;

// extra 30 bytes for the header over 1500
pub const NET_MTU: usize = 1530;
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

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum FlashOp {
    /// erase a region defined by (address, len)
    Erase(u32, u32),
    /// Send up to 1kiB of data at a time. This reduces messaging overhead and makes
    /// programming more efficient, while taking full advantage of the 1280-deep receive FIFO on the EC.
    /// Address + up to 4 pages. page 0 is at address, page 1 is at address + 256, etc.
    /// Pages stored as None are skipped, yet the address pointer is still incremented.
    Program(u32, [Option<[u8; 256]>; 4])
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) enum FlashResult {
    Pass,
    Fail,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct FlashRecord {
    /// identifier to validate that we're authorized to do this
    pub id: [u32; 4],
    /// operation
    pub op: FlashOp,
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct SsidRecord {
    pub name: xous_ipc::String::<32>,
    pub rssi: u8,
}
impl Default for SsidRecord {
    fn default() -> Self {
        SsidRecord { name: xous_ipc::String::<32>::new(), rssi: 0 }
    }
}
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct SsidReturn {
    pub list: [SsidRecord; 8],
}
impl Default for SsidReturn {
    fn default() -> Self {
        SsidReturn {
            list: [SsidRecord::default(); 8],
        }
    }
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Opcode {
    /// Reset the COM link - useful after an EC reset
    LinkReset,

    /// Refresh the TRNG seed for the EC
    ReseedTrng,

    /// Fetch the uptime of the EC
    GetUptime,

    /// Battery stats
    BattStats,

    /// Standby current -- only valid if a BattStats command was previously issued
    StandbyCurrent,

    /// Battery stats, non-blocking
    BattStatsNb,

    /// Query Full charge capacity of the battery
    //BattFullCapacity,

    /// More charger and gas gauge status, primarily for diagnostics
    MoreStats,

    /// Poll the USB CC chip
    PollUsbCc,

    /// Turn Boost Mode On
    BoostOn,

    /// Turn Boost Mode Off
    BoostOff,

    /// Read the current accelerations off the IMU; this blocks while the read takes place
    ImuAccelReadBlocking,

    /// Power off the SoC
    PowerOffSoc,

    /// Ship mode (battery disconnect)
    ShipMode,

    /// Is the battery charging?
    IsCharging,

    /// Set the backlight brightness
    SetBackLight,

    /// Request charging
    RequestCharging,

    /// Erase or program a region of EC FLASH
    FlashOp,

    /// Take the mutex on EC update operations.
    /// Only one process is allowed to acquire this ever, right now, for security reasons.
    FlashAcquire,

    /// Checks if an updated SSID list is available
    SsidCheckUpdate,

    /// Return the latest SSID list
    SsidFetchAsString,
    SsidFetchAsStringV2,

    /// Fetch the git ID of the EC
    EcGitRev,
    /// Fetch the SW tag of the EC as a {00|maj|min|rev} u32
    EcSwTag,

    /// Fetch the firmware rev of the WF200
    Wf200Rev,

    /// Send a line of PDS data
    Wf200PdsLine, //String<512>

    /// request for a listener to BattStats events
    RegisterBattStatsListener, //String<64>

    /// Reset the wifi chip
    Wf200Reset,

    /// Disable the wifi chip
    Wf200Disable,

    /// start passive SSID scanning
    ScanOn,

    /// stop passive SSID scanning
    ScanOff,

    /// suspend/resume callback
    SuspendResume,

    /// wlan: make sure radio is on (reset from standby if needed)
    WlanOn,

    /// wlan: switch radio to lowest power standby mode
    WlanOff,

    /// wlan: set SSID to use for joining AP
    WlanSetSSID,

    /// wlan: set password to use for joining AP
    WlanSetPass,

    /// wlan: join AP using previously set SSID & password
    WlanJoin,

    /// wlan: disconnect from AP
    WlanLeave,

    /// wlan: get wlan radio status (power state? connected? AP info?)
    WlanStatus,

    /// wlan: get current config
    WlanGetConfig,

    /// wlan: get net packet
    WlanFetchPacket,

    /// wlan: send net packet
    WlanSendPacket,

    /// sets the EC-side com interrupt mask
    IntSetMask,

    /// gets the EC-side com interrupt mask
    IntGetMask,

    /// acknowledges interrupts with the given mask
    IntAck,

    /// gets more details on the latest interrupt
    IntFetchVector,
}

/// These enums indicate what kind of callback type we're sending.
#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
pub(crate) enum Callback {
    /// Battery status
    BattStats,
    /// Server is quitting, drop connections
    Drop,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ComIntSources {
    WlanRxReady,
    WlanIpConfigUpdate,
    WlanSsidScanDone,
    BatteryCritical,
    WlanTxErr,
    WlanRxErr,
    Invalid,
}
impl From<u16> for ComIntSources {
    fn from(n: u16) -> ComIntSources {
        match n {
            com_rs_ref::INT_WLAN_RX_READY => ComIntSources::WlanRxReady,
            com_rs_ref::INT_WLAN_IPCONF_UPDATE => ComIntSources::WlanIpConfigUpdate,
            com_rs_ref::INT_WLAN_SSID_UPDATE => ComIntSources::WlanSsidScanDone,
            com_rs_ref::INT_BATTERY_CRITICAL => ComIntSources::BatteryCritical,
            com_rs_ref::INT_WLAN_TX_ERROR => ComIntSources::WlanTxErr,
            com_rs_ref::INT_WLAN_RX_ERROR => ComIntSources::WlanRxErr,
            _ => ComIntSources::Invalid,
        }
    }
}
impl From<ComIntSources> for u16 {
    fn from(cis: ComIntSources) -> u16 {
        match cis {
            ComIntSources::BatteryCritical => com_rs_ref::INT_BATTERY_CRITICAL,
            ComIntSources::WlanIpConfigUpdate => com_rs_ref::INT_WLAN_IPCONF_UPDATE,
            ComIntSources::WlanSsidScanDone => com_rs_ref::INT_WLAN_SSID_UPDATE,
            ComIntSources::WlanRxReady => com_rs_ref::INT_WLAN_RX_READY,
            ComIntSources::WlanTxErr => com_rs_ref::INT_WLAN_TX_ERROR,
            ComIntSources::WlanRxErr => com_rs_ref::INT_WLAN_RX_ERROR,
            ComIntSources::Invalid => 0,
        }
    }
}
