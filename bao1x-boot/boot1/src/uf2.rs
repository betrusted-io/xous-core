use core::convert::TryInto;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Uf2Block {
    pub magic_start0: u32, // should be 0x0A324655
    pub magic_start1: u32, // should be 0x9E5D5157
    pub flags: u32,
    pub target_addr: u32,
    pub payload_size: u32,
    pub block_no: u32,
    pub num_blocks: u32,
    pub file_size_family_id: u32, // may be zero
    pub data: [u8; 476],          // payload data
    pub magic_end: u32,           // should be 0x0AB16F30
}

impl Uf2Block {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 512 {
            return None;
        }

        let magic_start0 = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let magic_start1 = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let target_addr = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let payload_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let block_no = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let num_blocks = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let file_size_family_id = u32::from_le_bytes(bytes[28..32].try_into().unwrap());

        let mut data = [0u8; 476];
        data.copy_from_slice(&bytes[32..508]);

        let magic_end = u32::from_le_bytes(bytes[508..512].try_into().unwrap());

        if magic_start0 != 0x0A32_4655 || magic_start1 != 0x9E5D_5157 || magic_end != 0x0AB16F30 {
            None
        } else {
            Some(Uf2Block {
                magic_start0,
                magic_start1,
                flags,
                target_addr,
                payload_size,
                block_no,
                num_blocks,
                file_size_family_id,
                data,
                magic_end,
            })
        }
    }

    pub fn data(&self) -> &[u8] { &self.data[..self.payload_size as usize] }

    pub fn family(&self) -> u32 { self.file_size_family_id }

    pub fn address(&self) -> u32 { self.target_addr }
}
