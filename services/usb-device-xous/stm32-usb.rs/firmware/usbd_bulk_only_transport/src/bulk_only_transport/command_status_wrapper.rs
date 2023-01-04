use super::CommandStatus;
use packing::Packed;

/// Signature that identifies this packet as CSW
const SIGNATURE: u32 = 0x53425355;

/// A wrapper that identifies a command sent from the host to the
/// device on the OUT endpoint. Describes the data transfer IN or OUT
/// that should happen immediatly after this wrapper is received.
/// Little Endian
#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(little_endian, lsb0)]
pub struct CommandStatusWrapper {
    /// Signature that identifies this packet as CSW
    /// Must contain 0x53425355
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=3)]
    pub signature: u32,
    /// Tag that matches this CSW back to the CBW that initiated it.
    /// Must be copied from CBW tag field. Host uses it to positively 
    /// associate a CSW with the corresponding CBW
    #[packed(start_bit=7, end_bit=0, start_byte=4, end_byte=7)]
    pub tag: u32,
    /// Difference between the expected data length from CSW.data_transfer_length
    /// and the the actual amount of data sent or received. Cannot be greater
    /// than data_transfer_length. Non-zero for an OUT (host to device) transfer
    /// likely means there was an error whereas non-zero on IN (device to host) may
    /// mean the host allocated enough space for an extended/complete result but
    /// a shorter result was sent.
    #[packed(start_bit=7, end_bit=0, start_byte=8, end_byte=11)]
    pub data_residue: u32,
    /// The status of the command
    /// 0x00 = Command succeeded
    /// 0x01 = Command failed
    /// 0x02 = Phase error. Causes the host to perform a reset recovery on the 
    ///        device. This indicates the device state machine has got messed up
    ///        or similar unrecoverable condition. Processing further CBWs without
    ///        a reset gives indeterminate results.
    #[packed(start_bit=7, end_bit=0, start_byte=12, end_byte=12)]
    pub status: CommandStatus,
}

impl Default for CommandStatusWrapper {
    fn default() -> Self {
        Self {
            signature: SIGNATURE,
            tag: 0,
            data_residue: 0,
            status: CommandStatus::CommandOk,
        }
    }
}