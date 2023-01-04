use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct Verify10Command {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,
    
    #[pkd(7, 5, 1, 1)]
    pub vr_protect: u8,
    
    #[pkd(4, 4, 1, 1)]
    pub dpo: bool,
    
    #[pkd(1, 1, 1, 1)]
    pub byte_check: u8,
    
    #[pkd(7, 0, 2, 5)]
    pub lba: u32,
    
    #[pkd(4, 0, 6, 6)]
    pub group_number: u8,
    
    #[pkd(7, 0, 7, 8)]
    pub verification_length: u16,
    
    #[pkd(7, 0, 9, 9)]
    pub control: Control,
}
impl ParsePackedStruct for Verify10Command {}