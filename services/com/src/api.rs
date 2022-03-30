// NOTE: the use of ComState "verbs" as commands is not meant as a 1:1 mapping of commands
// It's just a convenient abuse of already-defined constants. However, it's intended that
// the COM server on the SoC side abstracts much of the EC bus complexity away.
pub(crate) const SERVER_NAME_COM: &str = "_COM manager_";
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
    Program(u32, [Option<[u8; 256]>; 4]),
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
pub struct SsidRecord {
    pub name: xous_ipc::String<32>,
    /// rssi is reported as the negative of actual rssi in dBm. Example: an rssi of -42dBm is reported as `42u8`.
    pub rssi: u8,
}
impl Default for SsidRecord {
    fn default() -> Self {
        SsidRecord {
            name: xous_ipc::String::<32>::new(),
            rssi: 0,
        }
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
#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct WlanStatusIpc {
    pub ssid: Option<SsidRecord>,
    pub link_state: u16, // this is slung around as a u16 to avoid pulling rkyv into the EC dependency tree
    pub ipv4: [u16; com_rs_ref::ComState::WLAN_GET_IPV4_CONF.r_words as usize],
}
impl WlanStatusIpc {
    #[allow(dead_code)]
    pub fn from_status(status: WlanStatus) -> Self {
        WlanStatusIpc {
            ssid: status.ssid,
            link_state: status.link_state as u16,
            ipv4: status.ipv4.encode_u16(),
        }
    }
}
impl Default for WlanStatusIpc {
    fn default() -> Self {
        WlanStatusIpc {
            ssid: None,
            link_state: com_rs_ref::LinkState::Unknown as u16,
            ipv4: [0u16; com_rs_ref::ComState::WLAN_GET_IPV4_CONF.r_words as usize],
        }
    }
}
#[derive(Debug, Copy, Clone)]
pub struct WlanStatus {
    pub ssid: Option<SsidRecord>,
    pub link_state: com_rs_ref::LinkState, // converted back into LinkState once it's across the IPC boundary
    pub ipv4: Ipv4Conf,
}
impl WlanStatus {
    #[allow(dead_code)]
    pub fn from_ipc(status: WlanStatusIpc) -> Self {
        WlanStatus {
            ssid: status.ssid,
            link_state: com_rs_ref::LinkState::decode_u16(status.link_state),
            ipv4: com_rs_ref::serdes::Ipv4Conf::decode_u16(&status.ipv4),
        }
    }
}

#[derive(Debug, Copy, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Default)]
pub struct WlanDebug {
    pub tx_errs: u32,
    pub drops: u32,
    // config(2) - control - alloc_fail(2) - alloc_oversize(2) - alloc_count
    pub config: u32,
    pub control: u16,
    pub alloc_fail: u32,
    pub alloc_oversize: u32,
    pub alloc_free_count: u16,
}

#[derive(Debug, num_derive::FromPrimitive, num_derive::ToPrimitive)]
#[repr(C)]
pub(crate) enum Opcode {
    /// A ping back, used to determine when the COM has finished initializing
    Ping = 0,

    /// Reset the COM link - useful after an EC reset
    LinkReset = 1,

    /// Refresh the TRNG seed for the EC
    ReseedTrng = 2,

    /// Fetch the uptime of the EC
    GetUptime = 3,

    /// Battery stats
    BattStats = 4,

    /// Standby current -- only valid if a BattStats command was previously issued
    StandbyCurrent = 5,

    /// Battery stats, non-blocking
    BattStatsNb = 6,

    /// Query Full charge capacity of the battery
    //BattFullCapacity = 7,

    /// More charger and gas gauge status, primarily for diagnostics
    MoreStats = 8,

    /// Poll the USB CC chip
    PollUsbCc = 9,

    /// Turn Boost Mode On
    BoostOn = 10,

    /// Turn Boost Mode Off
    BoostOff = 11,

    /// Read the current accelerations off the IMU; this blocks while the read takes place
    ImuAccelReadBlocking = 12,

    /// Power off the SoC
    PowerOffSoc = 13,

    /// Ship mode (battery disconnect)
    ShipMode = 14,

    /// Is the battery charging?
    IsCharging = 15,

    /// Set the backlight brightness
    SetBackLight = 16,

    /// Request charging
    RequestCharging = 17,

    /// Erase or program a region of EC FLASH
    FlashOp = 18,

    /// Take the mutex on EC update operations.
    /// Only one process is allowed to acquire this ever, right now, for security reasons.
    FlashAcquire = 19,

    /// Checks if an updated SSID list is available
    SsidCheckUpdate = 20,

    /// Return the latest SSID list
    SsidFetchAsString = 21,
    SsidFetchAsStringV2 = 22,

    /// Fetch the git ID of the EC
    EcGitRev = 23,
    /// Fetch the SW tag of the EC as a {00|maj|min|rev} u32
    EcSwTag = 24,

    /// Fetch the firmware rev of the WF200
    Wf200Rev = 25,

    /// Send a line of PDS data
    Wf200PdsLine = 26, //String<512>

    /// request for a listener to BattStats events
    RegisterBattStatsListener = 27, //String<64>

    /// Reset the wifi chip
    Wf200Reset = 28,

    /// Disable the wifi chip
    Wf200Disable = 29,

    /// start passive SSID scanning
    ScanOn = 30,

    /// stop passive SSID scanning
    ScanOff = 31,

    /// suspend/resume callback
    SuspendResume = 32,

    /// wlan: make sure radio is on (reset from standby if needed)
    WlanOn = 33,

    /// wlan: switch radio to lowest power standby mode
    WlanOff = 34,

    /// wlan: set SSID to use for joining AP
    WlanSetSSID = 35,

    /// wlan: set password to use for joining AP
    WlanSetPass = 36,

    /// wlan: join AP using previously set SSID & password
    WlanJoin = 37,

    /// wlan: disconnect from AP
    WlanLeave = 38,

    /// wlan: get wlan radio status (power state? connected? AP info?)
    WlanStatus = 39,

    /// wlan: get current config
    WlanGetConfig = 40,

    /// wlan: get net packet
    WlanFetchPacket = 41,

    /// wlan: send net packet
    WlanSendPacket = 42,

    /// wlan: debug infos
    WlanDebug = 43,

    /// wlan: get RSSI
    WlanRssi = 44,

    /// wlan: sync state (for resume)
    WlanSyncState = 45,

    /// sets the EC-side com interrupt mask
    IntSetMask = 46,

    /// gets the EC-side com interrupt mask
    IntGetMask = 47,

    /// acknowledges interrupts with the given mask
    IntAck = 48,

    /// gets more details on the latest interrupt
    IntFetchVector = 49,
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
    WlanSsidScanUpdate,
    WlanSsidScanFinished,
    BatteryCritical,
    WlanTxErr,
    WlanRxErr,
    Disconnect,
    Connect,
    WfxErr,
    Invalid,
}
impl From<u16> for ComIntSources {
    fn from(n: u16) -> ComIntSources {
        match n {
            com_rs_ref::INT_WLAN_RX_READY => ComIntSources::WlanRxReady,
            com_rs_ref::INT_WLAN_IPCONF_UPDATE => ComIntSources::WlanIpConfigUpdate,
            com_rs_ref::INT_WLAN_SSID_UPDATE => ComIntSources::WlanSsidScanUpdate,
            com_rs_ref::INT_WLAN_SSID_FINISHED => ComIntSources::WlanSsidScanFinished,
            com_rs_ref::INT_BATTERY_CRITICAL => ComIntSources::BatteryCritical,
            com_rs_ref::INT_WLAN_TX_ERROR => ComIntSources::WlanTxErr,
            com_rs_ref::INT_WLAN_RX_ERROR => ComIntSources::WlanRxErr,
            com_rs_ref::INT_WLAN_DISCONNECT => ComIntSources::Disconnect,
            com_rs_ref::INT_WLAN_CONNECT_EVENT => ComIntSources::Connect,
            com_rs_ref::INT_WLAN_WFX_ERR => ComIntSources::WfxErr,
            _ => ComIntSources::Invalid,
        }
    }
}
impl From<ComIntSources> for u16 {
    fn from(cis: ComIntSources) -> u16 {
        match cis {
            ComIntSources::BatteryCritical => com_rs_ref::INT_BATTERY_CRITICAL,
            ComIntSources::WlanIpConfigUpdate => com_rs_ref::INT_WLAN_IPCONF_UPDATE,
            ComIntSources::WlanSsidScanUpdate => com_rs_ref::INT_WLAN_SSID_UPDATE,
            ComIntSources::WlanSsidScanFinished => com_rs_ref::INT_WLAN_SSID_FINISHED,
            ComIntSources::WlanRxReady => com_rs_ref::INT_WLAN_RX_READY,
            ComIntSources::WlanTxErr => com_rs_ref::INT_WLAN_TX_ERROR,
            ComIntSources::WlanRxErr => com_rs_ref::INT_WLAN_RX_ERROR,
            ComIntSources::Connect => com_rs_ref::INT_WLAN_CONNECT_EVENT,
            ComIntSources::Disconnect => com_rs_ref::INT_WLAN_DISCONNECT,
            ComIntSources::WfxErr => com_rs_ref::INT_WLAN_WFX_ERR,
            ComIntSources::Invalid => 0,
        }
    }
}
