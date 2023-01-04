use packing::Packed;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
pub enum SenseKey {
    /// Indicates that there is no specific sense key information to be reported. This may occur for a successful command or for a command that receives CHECK CONDITION status because one of the FILEMARK , EOM , or ILI bits is set to one.
    NoSense =0x0,
    /// Indicates that the command completed successfully, with some recovery action performed by the device server. Details may be determined by examining the additional sense bytes and the INFORMATION field. When multiple recovered errors occur during one command, the choice of which error to report (e.g., first, last, most severe) is vendor specific.
    RecoveredError =0x1,
    /// Indicates that the logical unit is not accessible. Operator intervention may be required to correct this condition.
    NotReady =0x2,
    /// Indicates that the command terminated with a non-recovered error condition that may have been caused by a flaw in the medium or an error in the recorded data. This sense key may also be returned if the device server is unable to distinguish between a flaw in the medium and a specific hardware failure (i.e., sense key 4h).
    MediumError =0x3,
    /// Indicates that the device server detected a non-recoverable hardware failure (e.g., controller failure, device failure, or parity error) while performing the command or during a self test.
    HardwareError =0x4,
    /// Indicates that:
    /// a) the command was addressed to an incorrect logical unit number (see SAM-4);
    /// b) the command had an invalid task attribute (see SAM-4);
    /// c) the command was addressed to a logical unit whose current configuration prohibits
    /// processing the command;
    /// d) there was an illegal parameter in the CDB; or
    /// e) there was an illegal parameter in the additional parameters supplied as data for some
    /// commands (e.g., PERSISTENT RESERVE OUT).
    /// If the device server detects an invalid parameter in the CDB, it shall terminate the command without
    /// altering the medium. If the device server detects an invalid parameter in the additional parameters
    /// supplied as data, the device server may have already altered the medium.
    IllegalRequest =0x5,
    /// Indicates that a unit attention condition has been established (e.g., the removable medium may have been changed, a logical unit reset occurred). See SAM-4.
    UnitAttention =0x6,
    /// Indicates that a command that reads or writes the medium was attempted on a block that is protected. The read or write operation is not performed.
    DataProtect =0x7,
    /// Indicates that a write-once device or a sequential-access device encountered blank medium or format-defined end-of-data indication while reading or that a write-once device encountered a non-blank medium while writing.
    BlankCheck =0x8,
    /// This sense key is available for reporting vendor specific conditions.
    VendorSpecific =0x9,
    /// Indicates an EXTENDED COPY command was aborted due to an error condition on the source device, the destination device, or both (see 6.3.3).
    CopyAborted =0xA,
    /// Indicates that the device server aborted the command. The application client may be able to recover by trying the command again.
    AbortedCommand =0xB,
    /// Indicates that a buffered SCSI device has reached the end-of-partition and data may remain in the buffer that has not been written to the medium. One or more RECOVER BUFFERED DATA command(s) may be issued to read the unwritten data from the buffer. (See SSC-2.)
    VolumeOverflow =0xD,
    /// Indicates that the source data did not match the data read from the medium.
    Miscompare =0xE,
    /// Indicates there is completion sense data to be reported. This may occur for a successful command.
    Completed =0xF,
}
impl Default for SenseKey {
    fn default() -> Self {
        SenseKey::NoSense
    }
}