use packing::Packed;

use super::Direction;

/// Signature that identifies this packet as CBW
const SIGNATURE: u32 = 0x43425355;
const SIGNATURE_0: u8 = ((SIGNATURE >> 0) & 0xFF) as u8;
const SIGNATURE_1: u8 = ((SIGNATURE >> 8) & 0xFF) as u8;
const SIGNATURE_2: u8 = ((SIGNATURE >> 16) & 0xFF) as u8;
const SIGNATURE_3: u8 = ((SIGNATURE >> 24) & 0xFF) as u8;

#[derive(Packed, Clone, Copy, Eq, PartialEq, Debug)]
#[packed(little_endian, lsb0)]
/// A wrapper that identifies a command sent from the host to the
/// device on the OUT endpoint. Describes the data transfer IN or OUT
/// that should happen immediatly after this wrapper is received.
/// Little Endian
pub struct CommandBlockWrapper {
    /// Signature that identifies this packet as CBW
    /// Must contain 0x43425355
    #[packed(start_bit=7, end_bit=0, start_byte=0, end_byte=3)]
    pub signature: u32,
    /// Tag sent by the host. Must be echoed back to host in tag
    /// field of the command status wrapper sent after the command
    /// has been executed/rejected. Host uses it to positively 
    /// associate a CSW with the corresponding CBW
    #[packed(start_bit=7, end_bit=0, start_byte=4, end_byte=7)]
    pub tag: u32,
    /// Number of bytes of data that the host expects to receive on
    /// the IN or OUT endpoint (as indicated by the direction field) 
    /// during the execution of this command. If this field is zero, 
    /// must respond directly with CSW
    #[packed(start_bit=7, end_bit=0, start_byte=8, end_byte=11)]
    pub data_transfer_length: u32,
    /// Direction of transfer initiated by this command.
    /// 0b0XXXXXXX = OUT from host to device
    /// 0b1XXXXXXX = IN from device to host
    /// X bits are obsolete or reserved
    #[packed(start_bit=7, end_bit=0, start_byte=12, end_byte=12)]
    pub direction: Direction,
    /// The device Logical Unit Number (LUN) to which the command is
    /// for. For devices that don't support multiple LUNs the host will
    /// set this field to zero.
    /// Devices that don't support multiple LUNS must not ignore this 
    /// field and apply all commands to LUN 0, [see General Problems with Commands](http://janaxelson.com/device_errors.htm)
    #[packed(start_bit=7, end_bit=0, start_byte=13, end_byte=13)]
    pub lun: u8,
    /// The number of valid bytes in data field
    #[packed(start_bit=7, end_bit=0, start_byte=14, end_byte=14)]
    pub data_length: u8,
    /// The command set specific data for this command
    #[packed(start_bit=7, end_bit=0, start_byte=15, end_byte=30)]
    pub data: [u8; 16],
}

impl Default for CommandBlockWrapper {
    fn default() -> Self {
        Self {
            signature: SIGNATURE,
            tag: 0,
            data_transfer_length: 0,
            direction: Direction::HostToDevice,
            lun: 0,
            data_length: 0,
            data: [0; 16],
        }
    }
}

impl CommandBlockWrapper {
    fn check_signature(buf: &[u8]) -> bool {
        buf.len() >= 4 &&
        buf[0] == SIGNATURE_0 &&
        buf[1] == SIGNATURE_1 &&
        buf[2] == SIGNATURE_2 &&
        buf[3] == SIGNATURE_3 
    }

    fn find_signature(buf: &[u8]) -> Option<usize> {
        if buf.len() < 4 {
            return None;
        }
        for i in 0..buf.len().saturating_sub(4) {
            if Self::check_signature(&buf[i..(i+4)]) {
                return Some(i);
            } 
        }
        None
    }

    pub fn truncate_to_signature(buf: &mut [u8]) -> usize {
        let len = buf.len();
        if len < 4 {
            return len;
        }

        let sig_index = Self::find_signature(buf)
            // If we didn't find the signature, leave the last 3 bytes which might be the beginning
            // of the signature which we'll find next time around
            .unwrap_or(len - 3);

        for i in sig_index..len {
            buf[i-sig_index] = buf[i];
        }

        len - sig_index
    }
}

#[test]
fn test_truncate_to_signature() {
    const LEN: usize = 512;
    let mut buffer = [0; LEN];

    // have to leave the last 3 bytes in case they are the start of a sig
    assert_eq!(CommandBlockWrapper::truncate_to_signature(&mut buffer[..]), 3);

    buffer[LEN-3] = SIGNATURE_0;
    buffer[LEN-2] = SIGNATURE_1;
    buffer[LEN-1] = SIGNATURE_2;
    assert_eq!(CommandBlockWrapper::truncate_to_signature(&mut buffer[..]), 3);
    assert_eq!(buffer[0], SIGNATURE_0);
    assert_eq!(buffer[1], SIGNATURE_1);
    assert_eq!(buffer[2], SIGNATURE_2);

    let l = 100;
    let o = 50;
    buffer[o] = SIGNATURE_0;
    buffer[o+1] = SIGNATURE_1;
    buffer[o+2] = SIGNATURE_2;
    buffer[o+3] = SIGNATURE_3;
    assert_eq!(CommandBlockWrapper::find_signature(&buffer[..l]), Some(o));
    let new_len = l-o;
    // Should truncate down
    assert_eq!(CommandBlockWrapper::truncate_to_signature(&mut buffer[..l]), new_len);

    let old = buffer.clone();
    // Shouldn't truncate anything
    assert_eq!(CommandBlockWrapper::truncate_to_signature(&mut buffer[..new_len]), new_len);
    // Or modify the buffer
    for i in 0..LEN {
        assert_eq!(buffer[i], old[i]);
    }
}