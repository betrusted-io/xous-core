use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct RequestSenseCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,
    
    #[pkd(0, 0, 1, 1)]
    pub descriptor_format: bool,
    
    #[pkd(7, 0, 4, 4)]
    pub allocation_length: u8,
    
    #[pkd(7, 0, 5, 5)]
    pub control: Control,
}
impl ParsePackedStruct for RequestSenseCommand {}