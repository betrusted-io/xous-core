use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ReadFormatCapacitiesCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 5, 1, 1)]
    pub logical_unit_number: u8,
    
    #[pkd(7, 0, 7, 8)]
    pub allocation_length: u16,
    
    #[pkd(7, 0, 11, 11)]
    pub control: Control,
}
impl ParsePackedStruct for ReadFormatCapacitiesCommand {}