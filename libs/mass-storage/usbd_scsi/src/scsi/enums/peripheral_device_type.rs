use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum PeripheralDeviceType {
    /// Direct access block device (e.g., magnetic disk)
    DirectAccessBlock = 0x00,
    /// Sequential-access device (e.g., magnetic tape)
    SequentialAccess = 0x01,
    /// Printer device
    Printer = 0x02,
    /// Processor device
    Processor = 0x03,
    /// Write-once device (e.g., some optical disks)
    WriteOnce = 0x04,
    /// CD/DVD device
    CdDvd = 0x05,
    /// Optical memory device (e.g., some optical disks)
    OpticalMemory = 0x07,
    /// Media changer device (e.g., jukeboxes)
    MediaChanger = 0x08,
    /// Storage array controller device (e.g., RAID)
    StorageArrayController = 0x0C,
    /// Enclosure services device
    EnclosureServices = 0x0D,
    /// Simplified direct-access device (e.g., magnetic disk)
    SimplifiedDirectAccess = 0x0E,
    /// Optical card reader/writer device
    OpticaCardReaderWriter = 0x0F,
    /// Bridge Controller Commands
    BridgeController = 0x10,
    /// Object-based Storage Device
    ObjectBasedStorage = 0x11,
    /// Automation/Drive Interface
    AutomationInterface = 0x12,
    /// Security manager device
    SecurityManager = 0x13,
    /// Well known logical unit
    WellKnownLogicalUnit = 0x1E,
    /// Unknown or no device type
    UnknownOrNone = 0x1F,
}

impl Default for PeripheralDeviceType {
    fn default() -> Self {
        PeripheralDeviceType::DirectAccessBlock
    }
}
