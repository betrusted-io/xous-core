use packing::Packed;
use crate::scsi::{
    packing::ParsePackedStruct,
    commands::Control,
};

#[derive(Clone, Copy, Eq, PartialEq, Debug, Packed)]
#[packed(big_endian, lsb0)]
pub struct ReportLunsCommand {
    #[pkd(7, 0, 0, 0)]
    pub op_code: u8,

    #[pkd(7, 0, 2, 2)]
    pub select_report: u8,
    
    #[pkd(7, 0, 6, 9)]
    pub allocation_length: u32,
    
    #[pkd(7, 0, 11, 11)]
    pub control: Control,
}
impl ParsePackedStruct for ReportLunsCommand {}